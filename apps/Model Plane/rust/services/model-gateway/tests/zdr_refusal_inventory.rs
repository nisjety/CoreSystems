//! Every zero-data-retention refusal the gateway emits is typed.
//!
//! A refusal whose status is a bare `failed_precondition` or `permission_denied`
//! carries no catalogue kind, so a client cannot tell it from any other
//! precondition, and the English sentence is the only thing it can show. Such a
//! call fails this test, and the test names the file and line.
//!
//! The scan reads this crate's own `src` tree at run time (the directory is
//! always present when the test runs from the repository). Files whose names
//! mark them as tests are not scanned: they assert on refusals, they do not
//! make one.

use std::path::{Path, PathBuf};

fn source_files(directory: &Path, found: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(directory).expect("the crate's src directory exists");
    for entry in entries {
        let path = entry.expect("a readable directory entry").path();
        if path.is_dir() {
            source_files(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase();
            if !name.contains("test") {
                found.push(path);
            }
        }
    }
}

#[test]
fn every_zdr_refusal_in_the_gateway_carries_a_catalogue_kind() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    source_files(&src, &mut files);
    assert!(
        files.len() > 20,
        "the scan found only {} files under {}",
        files.len(),
        src.display()
    );

    let mut untyped = Vec::new();
    for file in &files {
        let source = std::fs::read_to_string(file).expect("a readable source file");
        for line in mp_contracts::zdr_refusal::untyped_zdr_refusal_lines(&source) {
            let relative = file
                .strip_prefix(&src)
                .unwrap_or(file)
                .display()
                .to_string();
            untyped.push(format!("{relative}:{line}"));
        }
    }
    assert!(
        untyped.is_empty(),
        "untyped zero-data-retention refusals (build them from mp_contracts::zdr_refusal::Refusal \
         so the status carries the catalogue kind): {untyped:#?}"
    );
}

/// EP-3 S5: the context-references refusal names a catalogue feature, so the typed body a ZDR turn
/// with references gets carries a key every client can name.
#[test]
fn the_context_references_refusal_is_a_catalogue_feature() {
    let key = model_gateway::context_refs::ZDR_FEATURE;
    assert_eq!(key, "context_references");
    assert!(mp_contracts::zdr_refusal::feature(key).is_some());
    let body = model_gateway::context_refs::ContextRefRefusal::zdr_refusal().json_body();
    assert_eq!(body["error"]["code"], mp_contracts::zdr_refusal::code::FEATURE_NOT_SUPPORTED);
    assert_eq!(body["error"]["feature"], key);
}
