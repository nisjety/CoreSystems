//! The log-field discipline lint for the Verevon gateway (WS4.F, gateway half).
//!
//! `src/zdr_log.rs` drops events that carry a sensitive field name while a ZDR
//! request is in scope. That only works if the code uses those names for what
//! they mean, and does not smuggle the same data in through a message or an
//! error text. This test scans the gateway's non-test source (`src/**/*.rs`) for
//! the ways a log line can still leak and fails when a count rises above the
//! committed baseline (`tests/fixtures/log-field-baseline.json`). It is the same
//! lint as Quarry's (`crates/quarry-edge/tests/log_field_discipline.rs`), adapted
//! to this crate's layout; the two are a documented contract, not shared code.
//!
//! The gateway is a binary crate, so the sensitive field list is read from its
//! golden (`tests/fixtures/zdr-sensitive-fields.golden.json`), which a unit test
//! in `src/zdr_log.rs` pins to `SENSITIVE_FIELDS` and to Quarry's own golden.
//!
//! Rules (each violation is counted per `file::rule`):
//! - R1  a sensitive field name (`url`, `query`, `title`, `host`, ...) in a
//!       `tracing` macro. Log a count, a class, a hash or an id instead.
//! - R2  a raw `error = %e` / `err = ?e` field not wrapped in `zdr_log::class*`:
//!       transport error text embeds the request URL (log `error_class`).
//! - R3  an interpolated message literal (`info!("failed for {url}")`): the data
//!       is in the message, where no field filter can see it.
//! - R4  `tokio::spawn` without `.in_current_span()` or `.instrument(..)` in a
//!       request-path file (`src/domains/`, `src/onboarding/`, `upstream.rs`,
//!       `middleware.rs`, `cache_admission.rs`): the task loses the span, so it
//!       loses the ZDR mark.
//! - R0  an exemption with no reason.
//!
//! A line is exempt with `// zdr-log-ok: <reason>` on the macro's first line or
//! the line above it. The reason is mandatory.
//!
//! The baseline may only shrink. The test fails when a count rises, when a new
//! `file::rule` appears, and when a count falls without the baseline being
//! lowered, so a fix cannot be silently undone. Regenerate it after a fix with
//! `LOG_FIELD_BASELINE_UPDATE=1 cargo test --test log_field_discipline`.
//!
//! Known limits, accepted: a field built by a helper, or a macro wrapped in
//! another macro, is not seen; a field named innocently that holds a URL is not
//! caught. This is a ratchet against the common mistakes, not a proof.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const BASELINE: &str = include_str!("fixtures/log-field-baseline.json");
const SENSITIVE_GOLDEN: &str = include_str!("fixtures/zdr-sensitive-fields.golden.json");

/// The field names the ZDR event filter drops (see `src/zdr_log.rs`).
fn sensitive_fields() -> &'static [String] {
    static FIELDS: OnceLock<Vec<String>> = OnceLock::new();
    FIELDS.get_or_init(|| {
        serde_json::from_str(SENSITIVE_GOLDEN).expect("golden is a JSON array of strings")
    })
}
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

/// The source with every `#[cfg(test)] mod name { .. }` block blanked out
/// (newlines kept, so line numbers still match the file). Unlike cutting the
/// file at the first test module, this still scans production code that comes
/// after one: `upstream.rs` and `domains/auth/public.rs` have test modules in
/// the middle of the file.
fn production_part(source: &str) -> String {
    let masked = mask(source);
    let mut out = source.as_bytes().to_vec();
    let lines: Vec<&str> = source.split('\n').collect();
    let mut offset = 0;
    for (index, line) in lines.iter().enumerate() {
        let is_test_mod = line.trim() == "#[cfg(test)]"
            && lines.get(index + 1).is_some_and(|next| {
                let next = next.trim_start();
                next.starts_with("mod ")
                    || next.starts_with("pub mod ")
                    || next.starts_with("pub(crate) mod ")
            });
        if is_test_mod {
            if let Some(relative) = masked[offset..].find('{') {
                let open = offset + relative;
                // `mod name;` (no body) is handled by `test_only_files`.
                if !masked[offset..open].contains(';') {
                    if let Some(close) = matching_paren(masked.as_bytes(), open) {
                        for byte in &mut out[offset..=close] {
                            if *byte != b'\n' {
                                *byte = b' ';
                            }
                        }
                    }
                }
            }
        }
        offset += line.len() + 1;
    }
    String::from_utf8(out).expect("blanking keeps utf-8 boundaries")
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

fn request_path_file(rel: &str) -> bool {
    rel.starts_with("src/domains/")
        || rel.starts_with("src/onboarding/")
        || matches!(
            rel,
            "src/upstream.rs" | "src/middleware.rs" | "src/cache_admission.rs"
        )
}

fn scan_source(rel: &str, source: &str) -> Vec<Violation> {
    let production = production_part(source);
    let source = production.as_str();
    let masked_text = mask(source);
    let masked = masked_text.as_bytes();
    let mut found = Vec::new();
    let push = |rule: &str, offset: usize, anchor: usize, found: &mut Vec<Violation>| {
        let line = line_of(source, offset);
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
            if head == "error" || head == "err" {
                if !value.contains("zdr_log::class") {
                    push("R2", piece_offset, name_start, &mut found);
                }
            } else if sensitive_fields().iter().any(|field| field == head) {
                push("R1", piece_offset, name_start, &mut found);
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

/// Files that are only compiled for tests: `#[cfg(test)] mod name;` in a parent
/// file means `name.rs` (next to it, or in the parent's own directory) is all
/// test code, which the lint does not scan.
fn test_only_files(files: &[PathBuf]) -> Vec<PathBuf> {
    let mut skipped = Vec::new();
    for file in files {
        let Ok(source) = std::fs::read_to_string(file) else {
            continue;
        };
        let source = source.replace("\r\n", "\n");
        let lines: Vec<&str> = source.lines().collect();
        let parent = file.parent().unwrap_or(Path::new(""));
        let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        for (index, line) in lines.iter().enumerate() {
            if line.trim() != "#[cfg(test)]" {
                continue;
            }
            let Some(next) = lines.get(index + 1).map(|l| l.trim()) else {
                continue;
            };
            let Some(name) = next
                .strip_prefix("mod ")
                .or_else(|| next.strip_prefix("pub(crate) mod "))
                .and_then(|rest| rest.strip_suffix(';'))
            else {
                continue;
            };
            let name = name.trim();
            skipped.push(parent.join(format!("{name}.rs")));
            skipped.push(parent.join(name).join("mod.rs"));
            skipped.push(parent.join(stem).join(format!("{name}.rs")));
            skipped.push(parent.join(stem).join(name).join("mod.rs"));
        }
    }
    skipped
}

fn scan_workspace() -> Vec<Violation> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    let skipped = test_only_files(&files);
    let mut violations = Vec::new();
    for file in files {
        if skipped.contains(&file) {
            continue;
        }
        let rel = format!(
            "src/{}",
            file.strip_prefix(&src)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        );
        let source = std::fs::read_to_string(&file).expect("read source");
        violations.extend(scan_source(&rel, &source.replace("\r\n", "\n")));
    }
    violations
}

#[test]
fn the_gateway_stays_within_the_log_field_baseline() {
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
    scan_source("src/domains/search.rs", source)
        .into_iter()
        .map(|violation| violation.key)
        .collect()
}

#[test]
fn log_field_lint_flags_a_new_url_field() {
    let found = keys("fn f(u: &str) {\n    tracing::info!(url = %u, \"fetched\");\n}\n");
    assert_eq!(found, vec!["src/domains/search.rs::R1"]);
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
        vec!["src/domains/search.rs::R2"]
    );
    assert!(
        keys("fn f(e: E) { tracing::warn!(error = zdr_log::class(&e), \"failed\"); }\n").is_empty()
    );
}

#[test]
fn log_field_lint_flags_an_interpolated_message() {
    assert_eq!(
        keys("fn f(u: &str) { tracing::info!(\"failed for {u}\"); }\n"),
        vec!["src/domains/search.rs::R3"]
    );
    assert!(keys("fn f() { tracing::info!(\"literal {{braces}} only\"); }\n").is_empty());
}

#[test]
fn log_field_lint_flags_a_spawn_that_drops_the_span_in_a_request_path_file() {
    assert_eq!(
        keys("fn f() { tokio::spawn(async move { work().await }); }\n"),
        vec!["src/domains/search.rs::R4"]
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
        "src/gdpr_nats.rs",
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
fn log_field_lint_rejects_an_empty_reason() {
    let source =
        "fn f(u: &str) {\n    // zdr-log-ok:\n    tracing::info!(url = %u, \"probe\");\n}\n";
    assert_eq!(keys(source), vec!["src/domains/search.rs::R0"]);
}

#[test]
fn log_field_lint_scans_production_code_after_a_test_module() {
    let source = "fn f() {}\n#[cfg(test)]\nmod tests {\n    fn t(u: &str) { tracing::info!(url = %u, \"x\"); }\n}\nfn g(u: &str) {\n    tracing::info!(url = %u, \"y\");\n}\n";
    let found = scan_source("src/domains/search.rs", source);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].line, 7, "line numbers still match the file");
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
