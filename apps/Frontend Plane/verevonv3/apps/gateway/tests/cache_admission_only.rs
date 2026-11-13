//! Tripwire: content handlers reach the result cache only through admission.
//!
//! The gateway's result cache holds identity and posture entries AND content
//! entries (search results, page previews). Content entries are built from what
//! an upstream returned for a caller, so they must be keyed by org, never stored
//! or served for a Zero-Data-Retention caller, and never reused across a
//! different request shape. `src/cache_admission.rs` decides all of that; this
//! test fails the build if a handler under `src/domains/` reaches the raw
//! (unscoped) cache calls directly instead of `lookup_scoped` / `store_scoped`
//! with a key from `CacheAdmission::key`.
//!
//! This is a source scan, not a data-flow prover (the same accepted tradeoff as
//! `ssrf_forward_not_fetch.rs`).
//!
//! ## If this test fails
//!
//! Do not delete or loosen it. Either the new call site stores content: resolve
//! a `CacheAdmission` and use `lookup_scoped` / `store_scoped`; or it caches
//! identity or posture state keyed by the subject: add the file to
//! `ALLOWED_FILES` with a comment saying why it cannot hold tenant content.

use std::fs;
use std::path::{Path, PathBuf};

/// Raw cache calls that are not tenant-scoped. Matched after all whitespace is
/// removed, so a rustfmt-wrapped `state\n    .cache\n    .store(` is still caught.
const RAW_CALLS: &[&str] = &[
    ".cache.store(",
    ".cache.store_for_secs(",
    ".cache.lookup_within(",
    ".cache.lookup(",
];

/// Files under `src/domains/` allowed to use the raw calls, and why.
const ALLOWED_FILES: &[&str] = &[
    // Session and membership ownership decisions, keyed by the subject.
    "browser.rs",
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read src dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// File names of the modules a source declares only under `#[cfg(test)]`: `mod name;` gives `name.rs`,
/// and `#[path = "file.rs"] mod name;` gives `file.rs`. Such a file is test code, as the part of a source
/// after its first `#[cfg(test)]` is, so it may seed the cache directly.
fn cfg_test_module_files(compact: &str, out: &mut Vec<String>) {
    let mut rest = compact;
    while let Some(at) = rest.find("#[cfg(test)]") {
        rest = &rest[at + "#[cfg(test)]".len()..];
        let mut decl = rest;
        let mut path_file = None;
        if let Some(after) = decl.strip_prefix("#[path=\"") {
            if let Some(end) = after.find("\"]") {
                path_file = after[..end].rsplit('/').next().map(str::to_owned);
                decl = &after[end + 2..];
            }
        }
        let decl = decl
            .strip_prefix("pub(crate)")
            .or_else(|| decl.strip_prefix("pub"))
            .unwrap_or(decl);
        if let Some(after) = decl.strip_prefix("mod") {
            if let Some(end) = after.find(';') {
                let name = &after[..end];
                if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    out.push(path_file.unwrap_or_else(|| format!("{name}.rs")));
                }
            }
        }
    }
}

#[test]
fn content_cache_is_only_reached_through_admission() {
    let domains = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/domains");
    let mut files = Vec::new();
    rust_files(&domains, &mut files);
    assert!(!files.is_empty(), "no sources found under {domains:?}");

    let mut test_modules = Vec::new();
    for file in &files {
        let source = fs::read_to_string(file).expect("read source");
        let compact: String = source.chars().filter(|c| !c.is_whitespace()).collect();
        cfg_test_module_files(&compact, &mut test_modules);
    }

    let mut violations = Vec::new();
    for file in files {
        let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if ALLOWED_FILES.contains(&name) || test_modules.iter().any(|m| m == name) {
            continue;
        }
        let source = fs::read_to_string(&file).expect("read source");
        // Tests may seed the cache directly; only production code is scanned.
        let production = source.split("#[cfg(test)]").next().unwrap_or("");
        let compact: String = production.chars().filter(|c| !c.is_whitespace()).collect();
        for call in RAW_CALLS {
            if compact.contains(call) {
                violations.push(format!("{}: {}", file.display(), call));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "content handlers must use CacheAdmission + lookup_scoped/store_scoped:\n{}",
        violations.join("\n")
    );
}
