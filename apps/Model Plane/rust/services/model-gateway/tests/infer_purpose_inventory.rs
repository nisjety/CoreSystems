//! Every inference call says what it is for.
//!
//! The `purpose` label on the inference metrics is only as honest as the calls
//! that feed it: a request built without one is counted as `unspecified`, and
//! nothing else flags it. This test reads the production source of the three
//! crates that build inference requests (this one, execution-core and
//! session-core) and fails on any `InferRequest` literal that does not name a
//! purpose, naming the file and line.
//!
//! Test code is not scanned. A `#[cfg(test)]` item, and a file whose name marks
//! it as a test, builds requests to assert on them, not to make a call. A request
//! copied from another with `..` is still a literal here, so it names its own
//! purpose too.

use std::path::{Path, PathBuf};

/// The source directories whose production code makes inference calls, relative
/// to this crate.
const SOURCE_DIRECTORIES: [&str; 3] = ["src", "../execution-core/src", "../session-core/src"];

/// The type whose literals this test reads. The name is matched whole, so a type
/// such as `ProviderInferRequest` is not one.
const REQUEST: &str = "InferRequest";

/// Words that put `InferRequest {` in a declaration or a pattern rather than a
/// construction. A return type (`-> InferRequest {`) is handled separately.
const NOT_A_CONSTRUCTION: [&str; 6] = ["struct", "impl", "enum", "trait", "for", "let"];

/// Whether a file's name marks it as test code. The name is read by its words, so
/// `attestation.rs` is production code and not a test.
fn is_test_file(name: &str) -> bool {
    name.split(['_', '.'])
        .any(|word| matches!(word, "test" | "tests" | "testing" | "double" | "doubles"))
}

fn rust_files(directory: &Path, found: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(directory).expect("the source directory exists");
    for entry in entries {
        let path = entry.expect("a readable directory entry").path();
        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase();
            if !is_test_file(&name) {
                found.push(path);
            }
        }
    }
}

fn is_ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Where the string, comment, raw string or character literal that starts at
/// `at` ends. Any other byte ends one byte on, so the caller can step past it.
fn literal_end(bytes: &[u8], at: usize) -> usize {
    match bytes[at] {
        b'"' => {
            let mut index = at + 1;
            while index < bytes.len() && bytes[index] != b'"' {
                index += if bytes[index] == b'\\' { 2 } else { 1 };
            }
            index + 1
        }
        b'/' if bytes.get(at + 1) == Some(&b'/') => bytes[at..]
            .iter()
            .position(|&byte| byte == b'\n')
            .map_or(bytes.len(), |offset| at + offset),
        b'/' if bytes.get(at + 1) == Some(&b'*') => bytes[at + 2..]
            .windows(2)
            .position(|pair| pair == b"*/")
            .map_or(bytes.len(), |offset| at + 2 + offset + 2),
        b'r' if at == 0 || !is_ident(bytes[at - 1]) => {
            let hashes = bytes[at + 1..]
                .iter()
                .take_while(|&&byte| byte == b'#')
                .count();
            if bytes.get(at + 1 + hashes) != Some(&b'"') {
                return at + 1;
            }
            let closing = format!("\"{}", "#".repeat(hashes)).into_bytes();
            let body = at + 2 + hashes;
            bytes[body..]
                .windows(closing.len())
                .position(|window| window == closing.as_slice())
                .map_or(bytes.len(), |offset| body + offset + closing.len())
        }
        b'\'' if bytes.get(at + 1) == Some(&b'\\') => bytes[at + 2..]
            .iter()
            .position(|&byte| byte == b'\'')
            .map_or(bytes.len(), |offset| at + 2 + offset + 1),
        // A character literal closes within four bytes of UTF-8; a lifetime never does.
        b'\'' => (2..=5)
            .find(|&width| bytes.get(at + width) == Some(&b'\''))
            .map_or(at + 1, |width| at + width + 1),
        _ => at + 1,
    }
}

/// Where the item after a `#[cfg(test)]` attribute ends: after its `;` when it
/// has no body, otherwise after the `}` that closes its first `{`.
fn item_end(bytes: &[u8], from: usize) -> usize {
    let mut braces = 0usize;
    let mut parens = 0usize;
    let mut opened = false;
    let mut index = from;
    while index < bytes.len() {
        match bytes[index] {
            b'{' => {
                braces += 1;
                opened = true;
            }
            b'}' => {
                braces = braces.saturating_sub(1);
                if opened && braces == 0 {
                    return index + 1;
                }
            }
            b'(' | b'[' => parens += 1,
            b')' | b']' => parens = parens.saturating_sub(1),
            b';' if !opened && parens == 0 => return index + 1,
            b'"' | b'\'' | b'/' | b'r' => {
                let end = literal_end(bytes, index);
                if end > index + 1 {
                    index = end;
                    continue;
                }
            }
            _ => {}
        }
        index += 1;
    }
    bytes.len()
}

/// The source with every `#[cfg(test)]` item blanked out. Only the bytes of the
/// item change and its newlines stay, so a line of the result is the same line of
/// the file.
fn production_source(source: &str) -> String {
    const MARK: &str = "#[cfg(test)]";
    let bytes = source.as_bytes();
    let mut blanked = bytes.to_vec();
    let mut from = 0;
    while let Some(found) = source[from..].find(MARK) {
        let start = from + found;
        let line_start = source[..start].rfind('\n').map_or(0, |newline| newline + 1);
        if source[line_start..start].trim().is_empty() {
            let end = item_end(bytes, start + MARK.len());
            for byte in &mut blanked[start..end] {
                if *byte != b'\n' {
                    *byte = b' ';
                }
            }
            from = end;
        } else {
            from = start + MARK.len();
        }
    }
    String::from_utf8(blanked).expect("blanking writes only ASCII spaces over whole items")
}

/// Whether the text before `InferRequest {` makes it a declaration, a return type
/// or a pattern, none of which builds a request.
fn is_declaration(before: &str) -> bool {
    let mut head = before.trim_end();
    // A path such as `mp_contracts::model_plane::v1::` names the same type.
    while let Some(rest) = head.strip_suffix("::") {
        head = rest
            .trim_end_matches(|c: char| c.is_ascii_alphanumeric() || c == '_')
            .trim_end();
    }
    head.ends_with("->")
        || NOT_A_CONSTRUCTION.iter().any(|keyword| {
            head.strip_suffix(keyword).is_some_and(|rest| {
                !rest.ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
            })
        })
}

/// The byte offset of the `{` of every `InferRequest` construction in `text`.
fn constructions(text: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    for (start, _) in text.match_indices(REQUEST) {
        let end = start + REQUEST.len();
        let line_start = text[..start].rfind('\n').map_or(0, |newline| newline + 1);
        if (start > 0 && is_ident(bytes[start - 1]))
            || bytes.get(end).is_some_and(|&byte| is_ident(byte))
            || text[line_start..start].contains("//")
        {
            continue;
        }
        let rest = &text[end..];
        let open = end + (rest.len() - rest.trim_start().len());
        if bytes.get(open) != Some(&b'{') || is_declaration(&text[..start]) {
            continue;
        }
        found.push(open);
    }
    found
}

/// Whether the literal whose `{` is at `open` names a `purpose` field of its own.
fn names_a_purpose(text: &str, open: usize) -> bool {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    // The last significant byte at the literal's own level: a field name follows
    // its `{` or a `,`.
    let mut last = b'{';
    let mut index = open + 1;
    while index < bytes.len() {
        let byte = bytes[index];
        let comment = byte == b'/' && matches!(bytes.get(index + 1), Some(&(b'/' | b'*')));
        if byte.is_ascii_whitespace() {
            index += 1;
        } else if (comment || matches!(byte, b'"' | b'\'' | b'r'))
            && literal_end(bytes, index) > index + 1
        {
            if !comment && depth == 0 {
                last = b'"';
            }
            index = literal_end(bytes, index);
        } else if is_ident(byte) {
            let end = index
                + bytes[index..]
                    .iter()
                    .take_while(|&&byte| is_ident(byte))
                    .count();
            if depth == 0 && &text[index..end] == "purpose" && matches!(last, b'{' | b',') {
                return true;
            }
            if depth == 0 {
                last = b'a';
            }
            index = end;
        } else {
            match byte {
                b'{' | b'(' | b'[' => {
                    if depth == 0 {
                        last = byte;
                    }
                    depth += 1;
                }
                b'}' | b')' | b']' => {
                    if depth == 0 {
                        return false;
                    }
                    depth -= 1;
                    if depth == 0 {
                        last = byte;
                    }
                }
                _ => {
                    if depth == 0 {
                        last = byte;
                    }
                }
            }
            index += 1;
        }
    }
    false
}

#[test]
fn every_non_test_infer_request_names_a_purpose() {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for directory in SOURCE_DIRECTORIES {
        rust_files(&crate_root.join(directory), &mut files);
    }
    assert!(
        files.len() > 50,
        "the scan found only {} source files",
        files.len()
    );

    let mut found = 0;
    let mut unnamed = Vec::new();
    for file in &files {
        let source = std::fs::read_to_string(file).expect("a readable source file");
        let production = production_source(&source);
        for open in constructions(&production) {
            found += 1;
            if !names_a_purpose(&production, open) {
                let line = production[..open].matches('\n').count() + 1;
                unnamed.push(format!("{}:{line}", file.display()));
            }
        }
    }
    assert!(
        found >= 20,
        "the scan found only {found} production InferRequest literals, so the scanner has gone blind"
    );
    assert!(
        unnamed.is_empty(),
        "these InferRequest literals name no purpose (set `purpose` to what the call is for, \
         so the inference metrics count it under its own label): {unnamed:#?}"
    );
}
