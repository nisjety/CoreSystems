//! Log hygiene ratchet (WS12.S6): nothing page-derived reaches `tracing`.
//!
//! The WS12 redaction module masks secrets in what Quarry returns, stores and
//! emits, but only where a call site applies it. This test scans every crate's
//! production source (`crates/*/src/**/*.rs`, with `#[cfg(test)]` items blanked)
//! for `tracing` macro calls that log a value a page or a caller supplied:
//!
//! - `?action`, `?request`, `?observation`: a whole action, request or
//!   observation printed with `Debug` (its `Debug` carries typed text and page
//!   content);
//! - `.text` and `.value`: a typed string or a field value read from a page;
//! - `dom_summary` and `accessibility`: the page snapshot itself;
//! - `url = %x` where `x` is not wrapped in `redact_url(..)` or `log_url(..)`:
//!   a page or caller URL with its userinfo and secret query values.
//!
//! A call is exempt with `// zdr-log-ok: <reason>` on the macro's first line or
//! on the line above it. This is the convention of the WS4.F field lint in
//! `quarry-edge`, and the reason is mandatory: an empty reason does not exempt.
//! A reviewed exception that such a comment cannot express goes in
//! `tests/log_hygiene.allow`, one line per site: the path relative to `crates/`,
//! the trimmed source line and the reason, separated by tabs. An allow entry
//! that matches no violation fails the test, so the list cannot outlive the
//! code it excuses.
//!
//! This is a ratchet against the common mistakes, not a proof. It does not see a
//! field built by a helper, a macro wrapped in another macro, or a raw error text
//! (`error = %e`): a raw error text is the subject of WS4.F and of
//! `QuarryError::log_message()`, and the sites that need it call the helper.

use std::fs;
use std::path::{Path, PathBuf};

const ALLOW: &str = include_str!("log_hygiene.allow");
const LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error"];
const EXEMPT_TAG: &str = "zdr-log-ok:";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Violation {
    /// Path relative to `crates/`, forward slashes, for example `quarry-runtime/src/x.rs`.
    file: String,
    /// 1-based line of the banned token.
    line: usize,
    rule: &'static str,
    /// The trimmed source line of the token.
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AllowEntry {
    file: String,
    text: String,
    reason: String,
}

fn is_ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Replace comments and string-literal contents with spaces, keeping every byte
/// offset (and every newline) where it was, so the structure can be scanned
/// without being fooled by a `url = %` inside a comment or a string.
fn mask(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = bytes.to_vec();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    out[i] = b' ';
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                // Block comments are blanked without nesting; the code base has none that nest.
                let mut j = i + 2;
                while j < bytes.len() && !(bytes[j] == b'*' && bytes.get(j + 1) == Some(&b'/')) {
                    if bytes[j] != b'\n' {
                        out[j] = b' ';
                    }
                    j += 1;
                }
                let end = (j + 2).min(bytes.len());
                for byte in &mut out[i..end] {
                    if *byte != b'\n' {
                        *byte = b' ';
                    }
                }
                i = end;
            }
            b'r' if matches!(bytes.get(i + 1), Some(b'"') | Some(b'#'))
                && (i == 0 || (!is_ident(bytes[i - 1]))) =>
            {
                // A raw string, r"..." or r#"..."#.
                let mut j = i + 1;
                let mut hashes = 0;
                while bytes.get(j) == Some(&b'#') {
                    hashes += 1;
                    j += 1;
                }
                if bytes.get(j) != Some(&b'"') {
                    i += 1;
                    continue;
                }
                j += 1;
                let mut close = vec![b'"'];
                close.extend(std::iter::repeat(b'#').take(hashes));
                while j < bytes.len() && !bytes[j..].starts_with(&close) {
                    if bytes[j] != b'\n' {
                        out[j] = b' ';
                    }
                    j += 1;
                }
                i = (j + close.len()).min(bytes.len());
            }
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        if bytes[i] != b'\n' {
                            out[i] = b' ';
                        }
                        i += 1;
                    }
                    if bytes[i] != b'\n' {
                        out[i] = b' ';
                    }
                    i += 1;
                }
                i += 1;
            }
            b'\'' => {
                // A char literal ('"', '\'', 'a') or a lifetime. Skip a char literal.
                if bytes.get(i + 1) == Some(&b'\\') {
                    i += 2;
                    while i < bytes.len() && bytes[i] != b'\'' {
                        i += 1;
                    }
                    i += 1;
                } else if bytes.get(i + 2) == Some(&b'\'') {
                    i += 3;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    String::from_utf8(out).expect("masking writes ASCII spaces only")
}

/// Index of the bracket that closes the `(`, `[` or `{` at `open`, in masked text.
fn matching_close(masked: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (offset, byte) in masked[open..].iter().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// Blank every `#[cfg(test)]` item in masked text: up to its `;` when it has no
/// body, or through its matching `}` when it does. Newlines are kept.
fn blank_cfg_test(masked: &str) -> String {
    const TAG: &str = "#[cfg(test)]";
    let mut out = masked.as_bytes().to_vec();
    let mut from = 0;
    while let Some(rel) = masked[from..].find(TAG) {
        let at = from + rel;
        let after = at + TAG.len();
        let end = match (masked[after..].find('{'), masked[after..].find(';')) {
            (Some(brace), Some(semi)) if semi < brace => after + semi,
            (Some(brace), _) => {
                matching_close(masked.as_bytes(), after + brace).unwrap_or(masked.len() - 1)
            }
            (None, Some(semi)) => after + semi,
            (None, None) => masked.len() - 1,
        };
        for byte in &mut out[at..=end] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
        from = end + 1;
    }
    String::from_utf8(out).expect("blanking writes ASCII spaces only")
}

/// The offsets of the `tracing` macro calls in masked text, as
/// `(name_offset, open_paren, close_paren)`.
fn tracing_calls(masked: &str) -> Vec<(usize, usize, usize)> {
    let bytes = masked.as_bytes();
    let mut calls = Vec::new();
    for level in LEVELS {
        let needle = format!("{level}!");
        let mut from = 0;
        while let Some(rel) = masked[from..].find(&needle) {
            let at = from + rel;
            from = at + needle.len();
            // The name must start a path segment: `tracing::warn!` yes, `my_warn!` no.
            if at > 0 && is_ident(bytes[at - 1]) {
                continue;
            }
            let mut open = from;
            while open < bytes.len() && bytes[open].is_ascii_whitespace() {
                open += 1;
            }
            if bytes.get(open) != Some(&b'(') {
                continue;
            }
            if let Some(close) = matching_close(bytes, open) {
                calls.push((at, open, close));
            }
        }
    }
    calls.sort_unstable();
    calls
}

/// Offsets where `needle` occurs in `text`, optionally bounded by a non-identifier
/// byte on either side.
fn occurrences(text: &str, needle: &str, before_word: bool, after_word: bool) -> Vec<usize> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = text[from..].find(needle) {
        let at = from + rel;
        let end = at + needle.len();
        let before_ok = !before_word || at == 0 || !is_ident(bytes[at - 1]);
        let after_ok = !after_word || end >= bytes.len() || !is_ident(bytes[end]);
        if before_ok && after_ok {
            out.push(at);
        }
        from = end;
    }
    out
}

/// Whether the `url` at `after_name - 3` is assigned a bare `%` value: `url = %x`
/// where `x` is not `redact_url(..)` or `log_url(..)`.
fn bare_url_assignment(args: &str, after_name: usize) -> bool {
    let Some(rest) = args[after_name..].trim_start().strip_prefix('=') else {
        return false;
    };
    if rest.starts_with('=') {
        return false;
    }
    let Some(rest) = rest.trim_start().strip_prefix('%') else {
        return false;
    };
    let rest = rest.trim_start();
    !(rest.starts_with("redact_url(") || rest.starts_with("log_url("))
}

/// The banned tokens in one macro argument list, as `(rule, offset_in_args)`.
fn token_hits(args: &str) -> Vec<(&'static str, usize)> {
    let mut hits = Vec::new();
    let word_tokens: [(&'static str, &str, bool, bool); 7] = [
        ("?action", "?action", false, true),
        ("?request", "?request", false, true),
        ("?observation", "?observation", false, true),
        (".text", ".text", false, true),
        (".value", ".value", false, true),
        ("dom_summary", "dom_summary", true, true),
        ("accessibility", "accessibility", true, true),
    ];
    for (rule, needle, before_word, after_word) in word_tokens {
        for at in occurrences(args, needle, before_word, after_word) {
            hits.push((rule, at));
        }
    }
    for at in occurrences(args, "url", true, true) {
        if bare_url_assignment(args, at + "url".len()) {
            hits.push(("url = %", at));
        }
    }
    hits.sort_unstable_by_key(|(_, at)| *at);
    hits
}

/// Whether a source line carries `zdr-log-ok:` followed by a non-empty reason.
fn has_reasoned_exemption(line: &str) -> bool {
    line.find(EXEMPT_TAG)
        .is_some_and(|at| !line[at + EXEMPT_TAG.len()..].trim().is_empty())
}

/// 1-based line number of a byte offset in `source`.
fn line_of(source: &str, offset: usize) -> usize {
    source.as_bytes()[..offset]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1
}

/// Every violation in one file. `file` is the label the violations carry.
fn scan_source(file: &str, source: &str) -> Vec<Violation> {
    let masked = blank_cfg_test(&mask(source));
    let lines: Vec<&str> = source.lines().collect();
    let mut out = Vec::new();
    for (at, open, close) in tracing_calls(&masked) {
        let first_line = line_of(source, at);
        let exempt = [first_line.checked_sub(1), first_line.checked_sub(2)]
            .into_iter()
            .flatten()
            .filter_map(|index| lines.get(index))
            .any(|line| has_reasoned_exemption(line));
        if exempt {
            continue;
        }
        let args = &masked[open..=close];
        for (rule, offset) in token_hits(args) {
            let line = line_of(source, open + offset);
            let text = lines
                .get(line - 1)
                .map_or(String::new(), |l| l.trim().to_string());
            out.push(Violation {
                file: file.to_string(),
                line,
                rule,
                text,
            });
        }
    }
    out
}

/// Every `crates/<crate>/src/**/*.rs`, as `(label, path)`, sorted.
fn source_files(crates: &Path) -> Vec<(String, PathBuf)> {
    fn collect(dir: &Path, label: &str, out: &mut Vec<(String, PathBuf)>) {
        for entry in fs::read_dir(dir).expect("source directory is readable") {
            let entry = entry.expect("directory entry is readable");
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            let child = format!("{label}/{name}");
            if path.is_dir() {
                collect(&path, &child, out);
            } else if name.ends_with(".rs") {
                out.push((child, path));
            }
        }
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(crates).expect("crates directory is readable") {
        let entry = entry.expect("directory entry is readable");
        let krate = entry.file_name().to_string_lossy().into_owned();
        let src = entry.path().join("src");
        if src.is_dir() {
            collect(&src, &format!("{krate}/src"), &mut files);
        }
    }
    files.sort();
    files
}

fn parse_allow(raw: &str) -> Result<Vec<AllowEntry>, String> {
    let mut entries = Vec::new();
    for (index, line) in raw.lines().enumerate() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() != 3 || parts[2].trim().is_empty() {
            return Err(format!(
                "log_hygiene.allow line {}: want path, source line and reason, tab separated, with a non-empty reason",
                index + 1
            ));
        }
        entries.push(AllowEntry {
            file: parts[0].trim().to_string(),
            text: parts[1].trim().to_string(),
            reason: parts[2].trim().to_string(),
        });
    }
    Ok(entries)
}

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[test]
fn production_logs_carry_no_page_or_caller_text() {
    let allow = parse_allow(ALLOW).expect("log_hygiene.allow is well formed");
    let mut used = vec![false; allow.len()];
    let mut unallowed = Vec::new();
    for (label, path) in source_files(&crates_dir()) {
        let source =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        for violation in scan_source(&label, &source) {
            match allow
                .iter()
                .position(|entry| entry.file == violation.file && entry.text == violation.text)
            {
                Some(index) => used[index] = true,
                None => unallowed.push(violation),
            }
        }
    }
    assert!(
        unallowed.is_empty(),
        "log lines that may carry page or caller text (fix the call, wrap the value in redact_url/log_url, or add a reasoned zdr-log-ok comment):\n{unallowed:#?}"
    );
    let stale: Vec<&AllowEntry> = allow
        .iter()
        .zip(&used)
        .filter(|(_, used)| !**used)
        .map(|(entry, _)| entry)
        .collect();
    assert!(
        stale.is_empty(),
        "log_hygiene.allow entries that match no violation (remove them): {stale:#?}"
    );
}

#[test]
fn scanner_flags_each_banned_form() {
    let source = r#"
fn f() {
    tracing::info!(url = %url, "x");
    tracing::debug!(?action, "y");
    tracing::debug!(?request, "y");
    tracing::debug!(?observation, "y");
    tracing::warn!(text = %resp.text, "z");
    tracing::warn!(value = %field.value, "z");
    tracing::warn!(len = dom_summary.len(), "z");
    tracing::warn!(len = accessibility.len(), "z");
}
"#;
    let rules: Vec<&str> = scan_source("quarry-runtime/src/fixture.rs", source)
        .iter()
        .map(|v| v.rule)
        .collect();
    assert_eq!(
        rules,
        [
            "url = %",
            "?action",
            "?request",
            "?observation",
            ".text",
            ".value",
            "dom_summary",
            "accessibility",
        ]
    );
}

#[test]
fn scanner_passes_wrapped_urls_and_closed_labels() {
    let source = r#"
fn g(url: &Url, zdr: ZdrMode) {
    tracing::info!(url = %redact_url(url.as_str(), Profile::Durable, &mut t), "a");
    tracing::info!(url = %log_url(url.as_str()), "b");
    tracing::warn!(error_class = e.code.wire_name(), model = %model, "c");
    tracing::debug!(%reason, "d");
    tracing::info!(url = log_url(zdr, url), "e");
}
"#;
    assert!(scan_source("quarry-runtime/src/fixture.rs", source).is_empty());
}

#[test]
fn scanner_skips_test_items_and_needs_a_reasoned_exemption() {
    let source = r#"
#[cfg(test)]
mod tests {
    fn h() {
        tracing::info!(url = %url, "only in a test");
    }
}
fn k() {
    // zdr-log-ok: the operator-set endpoint, no caller data
    tracing::info!(url = %endpoint, "boot");
    // zdr-log-ok:
    tracing::info!(url = %endpoint, "an empty reason does not exempt");
}
"#;
    let violations = scan_source("quarry-runtime/src/fixture.rs", source);
    assert_eq!(violations.len(), 1, "{violations:#?}");
    assert_eq!(violations[0].rule, "url = %");
    assert!(violations[0].text.contains("an empty reason"));
    assert!(violations[0].line > 1, "{violations:#?}");
}

#[test]
fn allow_file_entries_are_well_formed() {
    let entries = parse_allow(
        "# header\n\nquarry-runtime/src/x.rs\tinfo!(url = %u)\tthe endpoint is operator set\n",
    )
    .expect("a well-formed entry parses");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].file, "quarry-runtime/src/x.rs");
    assert_eq!(entries[0].reason, "the endpoint is operator set");
    assert!(parse_allow("quarry-runtime/src/x.rs\tinfo!(url = %u)\t\n").is_err());
    assert!(parse_allow("quarry-runtime/src/x.rs only two fields\n").is_err());
}
