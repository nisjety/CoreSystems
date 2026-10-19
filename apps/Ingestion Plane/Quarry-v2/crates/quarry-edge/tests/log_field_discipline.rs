//! The log-field discipline lint (WS4.F).
//!
//! `quarry_edge::zdr_log` drops events that carry a sensitive field name while a
//! ZDR request is in scope. That only works if the code uses those names for
//! what they mean, and does not smuggle the same data in through a message or an
//! error text. This test scans every crate's non-test source for the ways a log
//! line can still leak and fails when a count rises above the committed baseline
//! (`tests/fixtures/log-field-baseline.json`).
//!
//! Rules (each violation is counted per `file::rule`):
//! - R1  a sensitive field name (`url`, `query`, `title`, `host`, ...) in a
//!       `tracing` macro. Log a count, a class, a hash or an id instead.
//! - R2  a raw `error = %e` / `err = ?e` field not wrapped in `zdr_log::class*`:
//!       transport error text embeds the request URL.
//! - R3  an interpolated message literal (`info!("failed for {url}")`): the data
//!       is in the message, where no field filter can see it.
//! - R4  `tokio::spawn` without `.in_current_span()` or `.instrument(..)` in a
//!       request-path file: the task loses the span, so it loses the ZDR mark.
//! - R5  an `error_class` field whose value is not a closed label. The event
//!       filter and R1/R2 key on field names, and `error_class` is a name they
//!       let through, so its value must be a string literal or come from
//!       `zdr_log::class*`, `ErrorCode::wire_name()`, `cdp_class(..)`,
//!       `.classify()`, `.kind()` or an `ErrorCode` (`?e.code`). An
//!       `error_class = %e` would put the error text past the filter.
//! - R6  a whole event, payload or body debug-printed (`?evt`, `evt = ?evt`):
//!       a `Debug` of a page event carries page text under a name no filter lists.
//!       (`?evt.event_type`, one closed field of it, is fine.) A bare `%e` or
//!       `?e` shorthand for an error is R2, like `error = %e`.
//! - R0  an exemption with no reason.
//!
//! A line is exempt with `// zdr-log-ok: <reason>` on the macro's first line or
//! the line above it. The reason is mandatory.
//!
//! A stretch of code is exempt with `// zdr-log-ok-begin: <reason>` ... `//
//! zdr-log-ok-end`: for code that provably never runs inside a request, such as
//! the boot and shutdown sequence in `main.rs` (about 46 sites, every one an
//! operator-configured URL or a startup error). The reason is mandatory, a
//! region that is never closed, and an end with no begin, are R0 violations.
//! Use it for a stretch where no request span can exist; use the per-line comment
//! for a single value that is not caller data.
//!
//! The baseline may only shrink. The test fails when a count rises, when a new
//! `file::rule` appears, and when a count falls without the baseline being
//! lowered, so a fix cannot be silently undone. Regenerate it after a fix with
//! `LOG_FIELD_BASELINE_UPDATE=1 cargo test -p quarry-edge --test log_field_discipline`.
//!
//! Known limits, accepted: a field built by a helper, or a macro wrapped in
//! another macro, is not seen; a field named innocently that holds a URL is not
//! caught. This is a ratchet against the common mistakes, not a proof.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use quarry_edge::zdr_log::SENSITIVE_FIELDS;

const BASELINE: &str = include_str!("fixtures/log-field-baseline.json");
const LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error"];

#[derive(Debug, Clone, PartialEq, Eq)]
struct Violation {
    key: String,
    line: usize,
    text: String,
}

/// Replace comments and string-literal contents with spaces, keeping every byte
/// offset (and newline) where it was, so structure can be scanned without being
/// fooled by `info!(` or `url =` inside a comment or a string.
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
                let mut depth = 0;
                while i < bytes.len() {
                    if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                        depth += 1;
                        out[i] = b' ';
                        out[i + 1] = b' ';
                        i += 2;
                    } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                        depth -= 1;
                        out[i] = b' ';
                        out[i + 1] = b' ';
                        i += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        if bytes[i] != b'\n' {
                            out[i] = b' ';
                        }
                        i += 1;
                    }
                }
            }
            b'r' if matches!(bytes.get(i + 1), Some(b'"') | Some(b'#'))
                && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'_') =>
            {
                // raw string r"..." / r#"..."#
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
                let close: Vec<u8> = std::iter::once(b'"')
                    .chain(std::iter::repeat(b'#').take(hashes))
                    .collect();
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
    String::from_utf8(out).expect("masking keeps utf-8 boundaries")
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Index of the `)` matching the `(` at `open` in masked text.
fn matching_paren(masked: &[u8], open: usize) -> Option<usize> {
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

/// Top-level comma-separated pieces of `masked[start..end]`, with their offsets.
fn split_args(masked: &[u8], start: usize, end: usize) -> Vec<(usize, usize)> {
    let mut pieces = Vec::new();
    let mut depth = 0i32;
    let mut piece_start = start;
    for index in start..end {
        match masked[index] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                pieces.push((piece_start, index));
                piece_start = index + 1;
            }
            _ => {}
        }
    }
    if piece_start < end {
        pieces.push((piece_start, end));
    }
    pieces
}

fn line_of(source: &str, offset: usize) -> usize {
    source[..offset]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

/// Whether `line` opens an inline module (`mod tests {`, `pub(crate) mod x {`),
/// as opposed to declaring a file module (`mod test_support;`).
fn opens_inline_mod(line: &str) -> bool {
    let line = line.trim();
    let line = line
        .strip_prefix("pub(crate) ")
        .or_else(|| line.strip_prefix("pub(super) "))
        .or_else(|| line.strip_prefix("pub "))
        .unwrap_or(line);
    line.starts_with("mod ") && !line.trim_end().ends_with(';')
}

/// The non-test part of a file: everything before the first `#[cfg(test)]`
/// that opens an inline `mod` block. A `#[cfg(test)] mod name;` declaration (a
/// test-only file module, as in `main.rs`) or a `#[cfg(test)]` item (a `use`, a
/// `fn`) does NOT end production code: cutting there hid every site after it.
/// Further attributes between the `cfg(test)` and the `mod` are skipped.
fn production_part(source: &str) -> &str {
    let mut offset = 0;
    let lines: Vec<&str> = source.split('\n').collect();
    for (index, line) in lines.iter().enumerate() {
        if line.trim() == "#[cfg(test)]" {
            let following = lines[index + 1..]
                .iter()
                .find(|next| !next.trim_start().starts_with("#["));
            if following.is_some_and(|next| opens_inline_mod(next)) {
                return &source[..offset];
            }
        }
        offset += line.len() + 1;
    }
    source
}

/// Exemption on this line or the one above: `Ok(true)` exempt with a reason,
/// `Ok(false)` none, `Err(())` an exemption with no reason.
fn exemption(source: &str, line: usize) -> Result<bool, ()> {
    let lines: Vec<&str> = source.split('\n').collect();
    for candidate in [line.checked_sub(1), line.checked_sub(2)]
        .into_iter()
        .flatten()
    {
        if let Some(text) = lines.get(candidate) {
            if let Some(position) = text.find("zdr-log-ok:") {
                let reason = text[position + "zdr-log-ok:".len()..].trim();
                return if reason.is_empty() { Err(()) } else { Ok(true) };
            }
        }
    }
    Ok(false)
}

/// Line ranges (1-based, inclusive) covered by `// zdr-log-ok-begin: <reason>` ...
/// `// zdr-log-ok-end`, and the lines of any malformed marker (no reason, a
/// begin inside a region, an end with no begin, a region never closed). A begin
/// with no reason does not open a region, so its sites stay visible.
fn exempt_regions(source: &str) -> (Vec<(usize, usize)>, Vec<usize>) {
    let mut regions = Vec::new();
    let mut malformed = Vec::new();
    let mut open: Option<usize> = None;
    for (index, text) in source.split('\n').enumerate() {
        let line = index + 1;
        let trimmed = text.trim_start();
        if !trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!") {
            continue;
        }
        if let Some(position) = trimmed.find("zdr-log-ok-begin:") {
            let reason = trimmed[position + "zdr-log-ok-begin:".len()..].trim();
            if reason.is_empty() || open.is_some() {
                malformed.push(line);
            } else {
                open = Some(line);
            }
        } else if trimmed.contains("zdr-log-ok-end") {
            match open.take() {
                Some(start) => regions.push((start, line)),
                None => malformed.push(line),
            }
        }
    }
    if let Some(start) = open {
        malformed.push(start);
    }
    (regions, malformed)
}

/// Whether an `error_class` value (masked: string contents are blanked, the
/// quotes stay) is a closed label that cannot carry caller data.
fn closed_label(value: &str) -> bool {
    let value = value.trim();
    value.starts_with('"')
        || value.contains("zdr_log::class")
        || value.contains("cdp_class(")
        || value.ends_with("wire_name()")
        || value.ends_with("classify()")
        || value.ends_with(".kind()")
        || value.ends_with(".code")
}

fn request_path_file(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    rel.starts_with("quarry-edge/src/")
        && (name.ends_with("_routes.rs") || name == "routes.rs" || rel.contains("/agent_routes/"))
}

fn scan_source(rel: &str, source: &str) -> Vec<Violation> {
    let source = production_part(source);
    let masked_text = mask(source);
    let masked = masked_text.as_bytes();
    let mut found = Vec::new();
    let (regions, malformed) = exempt_regions(source);
    for line in malformed {
        found.push(Violation {
            key: format!("{rel}::R0"),
            line,
            text: "zdr-log-ok-begin/end region without a reason, unbalanced or never closed".to_owned(),
        });
    }
    let push = |rule: &str, offset: usize, anchor: usize, found: &mut Vec<Violation>| {
        let line = line_of(source, offset);
        let anchor_line = line_of(source, anchor);
        if regions
            .iter()
            .any(|(start, end)| (*start..=*end).contains(&anchor_line) || (*start..=*end).contains(&line))
        {
            return;
        }
        // An exemption on the macro's first line (or above it) covers every field
        // in the macro; one on the field's own line (or above) covers that field.
        let verdict = match exemption(source, line_of(source, anchor)) {
            Ok(false) => exemption(source, line),
            other => other,
        };
        match verdict {
            Ok(true) => {}
            Ok(false) => found.push(Violation {
                key: format!("{rel}::{rule}"),
                line,
                text: source.lines().nth(line - 1).unwrap_or("").trim().to_owned(),
            }),
            Err(()) => found.push(Violation {
                key: format!("{rel}::R0"),
                line,
                text: "zdr-log-ok exemption without a reason".to_owned(),
            }),
        }
    };

    // Tracing macros.
    let mut search = 0;
    while let Some(relative) = masked_text[search..].find('!') {
        let bang = search + relative;
        search = bang + 1;
        let mut name_start = bang;
        while name_start > 0 && is_ident_byte(masked[name_start - 1]) {
            name_start -= 1;
        }
        let name = &masked_text[name_start..bang];
        if !LEVELS.contains(&name) {
            continue;
        }
        // `tracing::info!` or a bare `info!`, never `foo_info!` (the ident scan
        // already stopped at `_`-joined names being longer than the level).
        let mut open = bang + 1;
        while masked
            .get(open)
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            open += 1;
        }
        if masked.get(open) != Some(&b'(') {
            continue;
        }
        let Some(close) = matching_paren(masked, open) else {
            continue;
        };
        for (start, end) in split_args(masked, open + 1, close) {
            let piece = masked_text[start..end].trim();
            let piece_offset = start
                + (masked_text[start..end].len() - masked_text[start..end].trim_start().len());
            if piece.starts_with("target:") || piece.starts_with("parent:") {
                continue;
            }
            if piece.starts_with('"') {
                // The message literal (masked: quotes kept, contents blanked).
                let original = &source[piece_offset..piece_offset + piece.len()];
                let interpolated = original.replace("{{", "").replace("}}", "").contains('{');
                if interpolated {
                    push("R3", piece_offset, name_start, &mut found);
                }
                continue;
            }
            // `name = value`, `%ident`, `?ident` or a bare ident.
            let (field, value) = match piece.find('=') {
                Some(eq)
                    if !piece[eq..].starts_with("==")
                        && piece[..eq]
                            .trim()
                            .bytes()
                            .all(|b| is_ident_byte(b) || b == b'.') =>
                {
                    (
                        piece[..eq].trim().to_owned(),
                        piece[eq + 1..].trim().to_owned(),
                    )
                }
                _ => {
                    let bare = piece.trim_start_matches(['%', '?']);
                    let ident: String = bare
                        .chars()
                        .take_while(|c| is_ident_byte(*c as u8))
                        .collect();
                    (ident, bare.to_owned())
                }
            };
            let head = field.split('.').next().unwrap_or("");
            let named = piece.contains('=') && piece[..piece.find('=').unwrap()].trim() == field;
            if head == "error" || head == "err" || (!named && head == "e") {
                if !value.contains("zdr_log::class") {
                    push("R2", piece_offset, name_start, &mut found);
                }
            } else if head == "error_class" {
                if !closed_label(&value) {
                    push("R5", piece_offset, name_start, &mut found);
                }
            } else if SENSITIVE_FIELDS.contains(&head) {
                push("R1", piece_offset, name_start, &mut found);
            }
            // A whole event / payload / body, debug-printed.
            let printed = if named { value.as_str() } else { piece };
            if let Some(root) = printed.trim().strip_prefix('?') {
                if matches!(
                    root.trim(),
                    "evt" | "event" | "payload" | "body" | "resp" | "response" | "req" | "request"
                ) {
                    push("R6", piece_offset, name_start, &mut found);
                }
            }
        }
        search = close;
    }

    // Spawned tasks in request-path files.
    if request_path_file(rel) {
        let mut at = 0;
        while let Some(relative) = masked_text[at..].find("tokio::spawn(") {
            let start = at + relative;
            at = start + 1;
            let open = start + "tokio::spawn".len();
            let Some(close) = matching_paren(masked, open) else {
                continue;
            };
            let inner = masked_text[open + 1..close].trim_end();
            let carries_span = inner.ends_with(".in_current_span()")
                || inner.rfind(".instrument(").is_some_and(|position| {
                    matching_paren(inner.as_bytes(), position + ".instrument".len())
                        .is_some_and(|end| inner[end + 1..].trim().is_empty())
                });
            if !carries_span {
                push("R4", start, start, &mut found);
            }
        }
    }
    found
}

fn counts(violations: &[Violation]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for violation in violations {
        *counts.entry(violation.key.clone()).or_insert(0) += 1;
    }
    counts
}

/// `Ok` when `actual` equals `baseline`; otherwise a message naming each key
/// that rose (a new leak) or fell (lower the baseline).
fn ratchet(
    baseline: &BTreeMap<String, usize>,
    actual: &BTreeMap<String, usize>,
) -> Result<(), String> {
    let mut problems = Vec::new();
    for (key, count) in actual {
        let allowed = baseline.get(key).copied().unwrap_or(0);
        if *count > allowed {
            problems.push(format!(
                "{key}: {count} found, baseline allows {allowed} (a new violation)"
            ));
        }
    }
    for (key, allowed) in baseline {
        let count = actual.get(key).copied().unwrap_or(0);
        if count < *allowed {
            problems.push(format!(
                "{key}: {count} found, baseline says {allowed} (fixed: lower the baseline with LOG_FIELD_BASELINE_UPDATE=1)"
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

fn scan_workspace() -> Vec<Violation> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut crate_dirs: Vec<_> = std::fs::read_dir(&crates)
        .expect("crates directory")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.join("src").is_dir())
        .collect();
    crate_dirs.sort();
    let mut violations = Vec::new();
    for crate_dir in crate_dirs {
        let name = crate_dir
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let mut files = Vec::new();
        rust_files(&crate_dir.join("src"), &mut files);
        for file in files {
            let rel = format!(
                "{name}/src/{}",
                file.strip_prefix(crate_dir.join("src"))
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            );
            let source = std::fs::read_to_string(&file).expect("read source");
            violations.extend(scan_source(&rel, &source.replace("\r\n", "\n")));
        }
    }
    violations
}

#[test]
fn the_workspace_stays_within_the_log_field_baseline() {
    let violations = scan_workspace();
    let actual = counts(&violations);

    if std::env::var("LOG_FIELD_BASELINE_UPDATE").as_deref() == Ok("1") {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/log-field-baseline.json");
        let json = serde_json::to_string_pretty(&actual).unwrap() + "\n";
        std::fs::write(path, json).expect("write baseline");
        return;
    }

    let baseline: BTreeMap<String, usize> =
        serde_json::from_str(BASELINE).expect("baseline is JSON");
    if let Err(problems) = ratchet(&baseline, &actual) {
        let mut detail = String::new();
        for violation in &violations {
            let allowed = baseline.get(&violation.key).copied().unwrap_or(0);
            if actual[&violation.key] > allowed {
                detail.push_str(&format!(
                    "  {}:{}  {}\n",
                    violation.key, violation.line, violation.text
                ));
            }
        }
        panic!("log-field discipline:\n{problems}\n\nsites over the baseline:\n{detail}");
    }
}

// ---- the lint's own tests ------------------------------------------------

fn keys(source: &str) -> Vec<String> {
    scan_source("quarry-edge/src/search_routes.rs", source)
        .into_iter()
        .map(|violation| violation.key)
        .collect()
}

#[test]
fn log_field_lint_flags_a_new_url_field() {
    let found = keys("fn f(u: &str) {\n    tracing::info!(url = %u, \"fetched\");\n}\n");
    assert_eq!(found, vec!["quarry-edge/src/search_routes.rs::R1"]);
    // Shorthand and `?` forms too.
    assert_eq!(
        keys("fn f(url: &str) { tracing::debug!(%url, \"x\"); }\n").len(),
        1
    );
    assert_eq!(
        keys("fn f(query: &str) { warn!(?query, \"x\"); }\n").len(),
        1
    );
}

#[test]
fn log_field_lint_accepts_safe_fields_and_ignores_comments_and_strings() {
    let source = "fn f() {\n    // tracing::info!(url = %u, \"in a comment\");\n    let s = \"info!(url = %u)\";\n    tracing::info!(count = 3, status = 200, \"page extracted\");\n}\n";
    assert!(keys(source).is_empty());
}

#[test]
fn log_field_lint_flags_raw_error_text_and_accepts_a_class() {
    assert_eq!(
        keys("fn f(e: E) { tracing::warn!(error = %e, \"failed\"); }\n"),
        vec!["quarry-edge/src/search_routes.rs::R2"]
    );
    assert!(
        keys("fn f(e: E) { tracing::warn!(error = zdr_log::class(&e), \"failed\"); }\n").is_empty()
    );
}

#[test]
fn log_field_lint_flags_a_bare_error_shorthand() {
    for source in [
        "fn f(e: E) { tracing::warn!(%e, \"failed\"); }\n",
        "fn f(e: E) { tracing::warn!(?e, \"failed\"); }\n",
        "fn f(err: E) { tracing::warn!(%err, \"failed\"); }\n",
    ] {
        assert_eq!(
            keys(source),
            vec!["quarry-edge/src/search_routes.rs::R2"],
            "{source}"
        );
    }
}

#[test]
fn log_field_lint_accepts_error_class_only_with_a_closed_label() {
    for source in [
        "fn f(e: E) { warn!(error_class = \"serialize\", \"x\"); }\n",
        "fn f(e: E) { warn!(error_class = crate::zdr_log::class(&e), \"x\"); }\n",
        "fn f(e: E) { warn!(error_class = crate::zdr_log::class_reqwest(&e), \"x\"); }\n",
        "fn f(e: E) { warn!(error_class = e.code.wire_name(), \"x\"); }\n",
        "fn f(e: E) { warn!(error_class = crate::deadlines::cdp_class(&e), \"x\"); }\n",
        "fn f(e: E) { warn!(error_class = ?e.code, \"x\"); }\n",
        "fn f(e: E) { warn!(error_class = ?e.classify(), \"x\"); }\n",
        "fn f(e: E) { warn!(error_class = ?e.kind(), \"x\"); }\n",
    ] {
        assert!(keys(source).is_empty(), "{source}");
    }
}

#[test]
fn log_field_lint_flags_an_error_class_that_carries_text() {
    for source in [
        "fn f(e: E) { warn!(error_class = %e, \"x\"); }\n",
        "fn f(e: E) { warn!(error_class = ?e, \"x\"); }\n",
        "fn f(e: E) { warn!(error_class = e.to_string(), \"x\"); }\n",
        "fn f(e: E) { warn!(error_class = %e.message, \"x\"); }\n",
        "fn f(error_class: String) { warn!(%error_class, \"x\"); }\n",
    ] {
        assert_eq!(
            keys(source),
            vec!["quarry-edge/src/search_routes.rs::R5"],
            "{source}"
        );
    }
}

#[test]
fn log_field_lint_flags_a_debug_printed_event_and_accepts_one_closed_field_of_it() {
    assert_eq!(
        keys("fn f(evt: Ev) { tracing::warn!(?evt, \"x\"); }\n"),
        vec!["quarry-edge/src/search_routes.rs::R6"]
    );
    assert_eq!(
        keys("fn f(evt: Ev) { tracing::warn!(event = ?evt, \"x\"); }\n"),
        vec!["quarry-edge/src/search_routes.rs::R6"]
    );
    assert!(
        keys("fn f(evt: Ev) { tracing::warn!(event_type = ?evt.event_type, seq = evt.seq, \"x\"); }\n")
            .is_empty()
    );
}

#[test]
fn log_field_lint_flags_an_interpolated_message() {
    assert_eq!(
        keys("fn f(u: &str) { tracing::info!(\"failed for {u}\"); }\n"),
        vec!["quarry-edge/src/search_routes.rs::R3"]
    );
    assert!(keys("fn f() { tracing::info!(\"literal {{braces}} only\"); }\n").is_empty());
}

#[test]
fn log_field_lint_flags_a_spawn_that_drops_the_span_in_a_request_path_file() {
    assert_eq!(
        keys("fn f() { tokio::spawn(async move { work().await }); }\n"),
        vec!["quarry-edge/src/search_routes.rs::R4"]
    );
    assert!(
        keys("fn f() { tokio::spawn(async move { work().await }.in_current_span()); }\n")
            .is_empty()
    );
    assert!(
        keys("fn f() { tokio::spawn(async move { work().await }.instrument(span)); }\n").is_empty()
    );
    // Not a request-path file: not checked.
    assert!(scan_source(
        "quarry-runtime/src/publisher.rs",
        "fn f() { tokio::spawn(async { w().await }); }\n"
    )
    .is_empty());
}

#[test]
fn log_field_lint_accepts_a_zdr_log_ok_comment_with_a_reason() {
    let source = "fn f(u: &str) {\n    // zdr-log-ok: a health probe URL, never a caller's\n    tracing::info!(url = %u, \"probe\");\n}\n";
    assert!(keys(source).is_empty());
}

#[test]
fn log_field_lint_exemption_on_the_macro_line_covers_all_its_fields() {
    let source = "fn f(u: &str, t: &str) {\n    // zdr-log-ok: both are the operator's own config\n    tracing::info!(\n        url = %u,\n        title = %t,\n        \"probe\"\n    );\n}\n";
    assert!(keys(source).is_empty());
}

#[test]
fn log_field_lint_exempts_a_region_and_nothing_outside_it() {
    let source = "fn before(u: &str) { tracing::info!(url = %u, \"before\"); }\n// zdr-log-ok-begin: boot sequence, no request span exists\nfn boot(u: &str, e: E) {\n    tracing::info!(url = %u, \"configured\");\n    tracing::warn!(error = %e, \"failed\");\n}\n// zdr-log-ok-end\nfn after(u: &str) { tracing::info!(url = %u, \"after\"); }\n";
    let found = scan_source("quarry-edge/src/search_routes.rs", source);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[0].text.contains("before") && found[1].text.contains("after"), "{found:?}");
}

#[test]
fn log_field_lint_region_markers_must_be_well_formed() {
    // No reason: R0, and the region is not opened, so its site is still counted.
    let no_reason = "// zdr-log-ok-begin:\nfn f(u: &str) { tracing::info!(url = %u, \"x\"); }\n// zdr-log-ok-end\n";
    let mut found = keys(no_reason);
    found.sort();
    assert_eq!(
        found,
        vec![
            "quarry-edge/src/search_routes.rs::R0",
            "quarry-edge/src/search_routes.rs::R0",
            "quarry-edge/src/search_routes.rs::R1",
        ],
        "{no_reason}"
    );
    // Never closed.
    let unclosed = "// zdr-log-ok-begin: boot\nfn f() {}\n";
    assert_eq!(keys(unclosed), vec!["quarry-edge/src/search_routes.rs::R0"]);
    // An end with no begin.
    assert_eq!(keys("fn f() {}\n// zdr-log-ok-end\n"), vec!["quarry-edge/src/search_routes.rs::R0"]);
    // A begin inside a region.
    let nested = "// zdr-log-ok-begin: a\n// zdr-log-ok-begin: b\nfn f() {}\n// zdr-log-ok-end\n";
    assert_eq!(keys(nested), vec!["quarry-edge/src/search_routes.rs::R0"]);
    // Doc comments that merely mention the markers are not markers.
    assert!(keys("/// say zdr-log-ok-begin: x\n//! and zdr-log-ok-end\nfn f() {}\n").is_empty());
}

#[test]
fn log_field_lint_rejects_an_empty_reason() {
    let source =
        "fn f(u: &str) {\n    // zdr-log-ok:\n    tracing::info!(url = %u, \"probe\");\n}\n";
    assert_eq!(keys(source), vec!["quarry-edge/src/search_routes.rs::R0"]);
}

/// The defect this pins: `quarry-edge/src/main.rs` has `#[cfg(test)]` then
/// `mod test_support;` near its top, and the scanner treated that as the start of
/// the test code, so every macro after it (about 46 sites) was never scanned.
#[test]
fn log_field_lint_keeps_scanning_past_a_cfg_test_file_module_declaration() {
    let source = "mod a;\n#[cfg(test)]\nmod test_support;\n#[cfg(test)]\nuse x::y;\n#[cfg(test)]\n#[allow(dead_code)]\nfn helper() {}\nfn f(u: &str) { tracing::info!(url = %u, \"fetched\"); }\n";
    assert_eq!(keys(source), vec!["quarry-edge/src/search_routes.rs::R1"]);
    assert_eq!(production_part(source), source);
}

#[test]
fn log_field_lint_stops_at_the_first_inline_cfg_test_module_even_after_a_declaration() {
    let source = "#[cfg(test)]\nmod test_support;\nfn f(u: &str) { tracing::info!(url = %u, \"before\"); }\n#[cfg(test)]\n#[allow(clippy::all)]\nmod tests {\n    fn t(u: &str) { tracing::info!(url = %u, \"inside\"); }\n}\nfn g(u: &str) { tracing::info!(url = %u, \"after\"); }\n";
    let found = scan_source("quarry-edge/src/search_routes.rs", source);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].text.contains("before"), "{found:?}");
    for opener in ["mod tests {", "pub(crate) mod tests {", "pub mod t{", "mod t\n{"] {
        let line = opener.lines().next().unwrap();
        assert!(opens_inline_mod(line), "{opener}");
    }
    assert!(!opens_inline_mod("mod test_support;"));
    assert!(!opens_inline_mod("pub(crate) mod test_support;"));
    assert!(!opens_inline_mod("fn mod_x() {"));
}

/// The real file the defect hid: `main.rs` must be scanned to its last function,
/// not cut at the `#[cfg(test)] mod test_support;` declaration near its top.
#[test]
fn the_edge_main_is_scanned_past_its_test_support_declaration() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs");
    let source = std::fs::read_to_string(path).expect("main.rs").replace("\r\n", "\n");
    assert!(source.contains("#[cfg(test)]\nmod test_support;"), "premise: main.rs still declares test_support");
    let scanned = production_part(&source);
    assert!(scanned.contains("async fn run("), "the boot sequence is scanned");
    assert!(
        scanned.contains("pub fn routes_for_test"),
        "the scan reaches the end of main.rs, not the first cfg(test) item"
    );
}

#[test]
fn log_field_lint_ignores_test_modules() {
    let source = "fn f() {}\n#[cfg(test)]\nmod tests {\n    fn t(u: &str) { tracing::info!(url = %u, \"x\"); }\n}\n";
    assert!(keys(source).is_empty());
}

#[test]
fn log_field_lint_ratchet_fails_when_a_count_rises_or_falls_without_the_baseline() {
    let baseline: BTreeMap<String, usize> = [("a.rs::R1".to_owned(), 2)].into();
    let same: BTreeMap<String, usize> = [("a.rs::R1".to_owned(), 2)].into();
    let more: BTreeMap<String, usize> = [("a.rs::R1".to_owned(), 3)].into();
    let fewer: BTreeMap<String, usize> = [("a.rs::R1".to_owned(), 1)].into();
    let other: BTreeMap<String, usize> =
        [("a.rs::R1".to_owned(), 2), ("b.rs::R1".to_owned(), 1)].into();
    assert!(ratchet(&baseline, &same).is_ok());
    assert!(ratchet(&baseline, &more)
        .unwrap_err()
        .contains("a new violation"));
    assert!(ratchet(&baseline, &fewer)
        .unwrap_err()
        .contains("lower the baseline"));
    assert!(ratchet(&baseline, &other).unwrap_err().contains("b.rs::R1"));
}
