//! Every zero-data-retention refusal Execution Core emits is typed.
//!
//! A refusal whose status is a bare `failed_precondition` or `permission_denied`
//! carries no catalogue kind, so a client cannot tell it from any other
//! precondition. Such a call fails this test, and the test names the file and
//! line. Files whose names mark them as tests or test doubles are not scanned:
//! they assert on refusals, they do not make one.
//!
//! The named sites below are the inventory the typed refusals cover; the scan
//! reads this crate's own `src` tree at run time.

use std::path::{Path, PathBuf};

/// The sites that refuse ZDR work, by file, each built from the catalogue.
const NAMED_SITES: &[(&str, &str)] = &[
    ("grpc.rs", "enforce_persistence_free_execution"),
    ("grpc.rs", "scheduled-step service gate"),
    ("agent_route.rs", "validate_run_route"),
    ("runtime_loop/agent.rs", "continuation_descriptor"),
    ("workspace_promote.rs", "reject_zdr_durable_mutation"),
    ("auth.rs", "delegated inference bearer"),
    ("auth.rs", "delegated browser bearer"),
    ("auth.rs", "delegated Data Plane bearer"),
    ("auth.rs", "delegated sandbox bearer"),
    ("sandbox_lease.rs", "thread workspace zdr guard"),
];

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
fn every_zdr_refusal_in_execution_core_carries_a_catalogue_kind() {
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

#[test]
fn the_named_sites_are_typed_where_they_are_declared() {
    for (file, site) in NAMED_SITES {
        let source = std::fs::read_to_string(src_path(file)).expect("a named site's file exists");
        assert!(
            source.contains("zdr_refusal::Refusal"),
            "{file} ({site}) must build its ZDR refusal from mp_contracts::zdr_refusal::Refusal"
        );
    }
}

fn src_path(file: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(file)
}
