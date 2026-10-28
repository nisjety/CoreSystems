//! Every zero-data-retention refusal inference-core emits is typed.
//!
//! A refusal whose status is a bare `failed_precondition` or `permission_denied`
//! carries no catalogue kind, so a client cannot tell it from any other
//! precondition. Such a call fails this test, and the test names the file and
//! line. Files whose names mark them as tests or test doubles are not scanned.
//! The handler-by-handler inventory lives beside the handlers, in
//! `src/grpc.rs` (`zdr_refusal_catalogue`).

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
            if !name.contains("test") && !name.contains("double") {
                found.push(path);
            }
        }
    }
}

#[test]
fn every_zdr_refusal_in_inference_core_carries_a_catalogue_kind() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    source_files(&src, &mut files);
    assert!(
        files.len() > 10,
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
