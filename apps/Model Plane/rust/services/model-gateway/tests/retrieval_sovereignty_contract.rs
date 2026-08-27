//! Guards that EVERY Data Plane v2 retrieval request the Model Plane builds
//! declares a sovereignty posture.
//!
//! `RetrieveRequest.sovereign_required` is the axis Data Plane v2 resolves with
//! `unwrap_or(true)` — a deliberate fail-closed default, because absence of
//! proof is not proof that egress is permitted. The configured embedding
//! provider (Cohere Embed v4, Azure-hosted) can never satisfy that, so a request
//! that omits the field does not degrade: the dense arm fails outright, before
//! any retrieval happens, with a message that names ZDR and never mentions
//! sovereignty at all. Verified live 2026-08-27 against retrieval-engine:50052:
//! the identical query and token returns 10 candidates with
//! `sovereignRequired: false` and `Internal: ZDR content must not egress to the
//! Cohere Embed v4 text path` with the field absent.
//!
//! # Why this is a source-text test
//!
//! Because the type checker cannot see the bug, and a passing unit test did not
//! either. Data Plane v2's own fix for this shipped with green tests while the
//! live RPC still failed: the streaming path had been wired and the unary
//! handler carried a hand-maintained duplicate of the same field-by-field
//! mapping, still saying `sovereign_required: None`. One construction site was
//! fixed, the other was not, and nothing that compiles or runs could tell.
//!
//! The Model Plane has the same shape, worse. Of its five construction sites,
//! THREE use `..Default::default()` — which fills the field with `None` and
//! compiles forever. Only the two exhaustive literals would ever fail a build.
//! So this walks the workspace source and requires every `RetrieveRequest`
//! literal to set the field explicitly.
//!
//! If this fails on a new call site: set `sovereign_required` there. Derive it
//! (`mp_contracts::dataplane_posture::effective_sovereign_required`) if the site
//! has a signed claim or a caller-declared value to derive from; otherwise use
//! `SOVEREIGN_REQUIRED_WITHOUT_SIGNAL` and say in a comment what would have to
//! be threaded in to do better. Do not delete this test to make it pass — an
//! omitted field is not a smaller version of the bug, it is the bug.

use std::path::{Path, PathBuf};

/// The Model Plane's Rust workspace root, from this crate's manifest dir.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root is two levels above services/model-gateway")
}

/// Every `.rs` file under the workspace, excluding build output.
fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            // `target` is build output; a stale generated copy in there would
            // produce findings nobody can act on.
            if name != "target" && !name.starts_with('.') {
                rust_sources(&path, out);
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// Slice out one struct literal by brace balance, starting at `RetrieveRequest {`.
fn literal_at(text: &str, open_brace: usize) -> &str {
    let mut depth = 0usize;
    for (offset, ch) in text[open_brace..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &text[open_brace..=open_brace + offset];
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced braces in a RetrieveRequest literal");
}

#[test]
fn every_retrieve_request_declares_a_sovereignty_posture() {
    let root = workspace_root();
    let mut files = Vec::new();
    rust_sources(&root, &mut files);
    assert!(
        files.len() > 50,
        "only {} source files found under {} — this test is scanning the wrong \
         tree and would pass no matter what",
        files.len(),
        root.display()
    );

    let mut checked = 0usize;
    let mut offenders: Vec<String> = Vec::new();

    for file in &files {
        // This test's own prose names the type repeatedly; scanning it would
        // find no literals but the exclusion keeps the intent obvious.
        if file.ends_with("retrieval_sovereignty_contract.rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let mut from = 0usize;
        while let Some(found) = text[from..].find("RetrieveRequest {") {
            let start = from + found;
            // `RetrieveRequest {` also opens the generated `pub struct` and any
            // `impl`; both live under `target/`, which is excluded, but a
            // pattern match (`let RetrieveRequest { .. }`) would look the same.
            // Require an initializer: the previous non-space character being
            // `=`, `(`, `,` or `!` is what distinguishes one.
            let prefix = text[..start].trim_end();
            let is_initializer = prefix
                .chars()
                .last()
                .is_some_and(|ch| matches!(ch, '=' | '(' | ',' | '!' | '{'))
                || prefix.ends_with("ret_pb::")
                || prefix.ends_with("Request::new(");
            from = start + "RetrieveRequest {".len();
            if !is_initializer {
                continue;
            }
            // The `{` of `RetrieveRequest {` is the last byte of the needle.
            let open = start + "RetrieveRequest ".len();
            debug_assert_eq!(&text[open..=open], "{");
            let literal = literal_at(&text, open);
            checked += 1;
            if !literal.contains("sovereign_required") {
                let line = text[..start].matches('\n').count() + 1;
                offenders.push(format!(
                    "{}:{line}",
                    file.strip_prefix(&root).unwrap_or(file).display()
                ));
            }
        }
    }

    assert!(
        checked >= 5,
        "found only {checked} RetrieveRequest literals; the Model Plane had 5 when \
         this was written (model-gateway dataplane/retrieval/tool_loop, \
         execution-core knowledge_tools, session-core grpc). Fewer means the scan \
         broke, not that the call sites went away."
    );
    assert!(
        offenders.is_empty(),
        "these RetrieveRequest literals omit `sovereign_required`, so Data Plane \
         v2 will resolve them to the fail-closed default and every dense query \
         through them will fail before retrieving anything: {offenders:?}"
    );
}
