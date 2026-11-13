//! Every zero-data-retention refusal Session Core emits is typed (WS06-S1).
//!
//! A refusal whose status is a bare `failed_precondition` or `permission_denied` carries no
//! catalogue kind, so a client cannot tell it from any other precondition. Such a call fails
//! this test, and the test names the file and line. Files whose names mark them as tests or
//! test doubles are not scanned: they assert on refusals, they do not make one.
//!
//! The named sites below are the inventory WS06-S1 typed; the scan reads this crate's own
//! `src` tree at run time. The behaviour of each site (the kind, the feature and the gRPC code)
//! is pinned by the `zdr_refusal_*` unit tests beside it, because the sites are private to the
//! binary crate.

use std::path::{Path, PathBuf};

/// The sites that refuse ZDR work, by file, each built from the catalogue (16 in all).
const NAMED_SITES: &[(&str, &str)] = &[
    ("auth.rs", "authorize_operation write gate"),
    ("auth.rs", "authorize_space_deletion_service"),
    ("auth.rs", "authorize_scheduled_step_service"),
    ("auth.rs", "authorize_run_action_authority_service"),
    ("auth.rs", "authorize_scheduled_step_authority_service"),
    ("auth.rs", "authorize_system_run_owner"),
    ("auth.rs", "delegated Data Plane bearer posture mismatch"),
    ("grpc.rs", "record_scheduled_run_result_inner"),
    ("grpc.rs", "record_run_output"),
    ("grpc.rs", "persist_scheduled_run_bindings"),
    ("memory_grpc.rs", "authorize_memory_preflight"),
    ("memory_grpc.rs", "set_memory_controls"),
    ("message_search.rs", "recall search"),
    ("message_tree.rs", "set_active_leaf waived caller and waived thread"),
    ("terminalization.rs", "validate_managed_start_request"),
    ("terminalization.rs", "complete_direct_run_with_message_inner"),
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

fn src_path(file: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(file)
}

#[test]
fn every_zdr_refusal_in_session_core_carries_a_catalogue_kind() {
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
            let relative = file.strip_prefix(&src).unwrap_or(file).display().to_string();
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
    assert_eq!(
        NAMED_SITES.len(),
        16,
        "the WS06-S1 inventory plus message versions is 16 sites"
    );
    for (file, site) in NAMED_SITES {
        let source = std::fs::read_to_string(src_path(file)).expect("a named site's file exists");
        assert!(
            source.contains("zdr_refusal::Refusal") || source.contains("Refusal::"),
            "{file} ({site}) must build its ZDR refusal from mp_contracts::zdr_refusal::Refusal"
        );
        assert!(
            source.contains("DURABLE_MUTATION_FORBIDDEN")
                || source.contains("feature_not_supported")
                || source.contains("POSTURE_MISMATCH"),
            "{file} ({site}) names no catalogue refusal"
        );
    }
}

#[test]
fn the_pre_typing_messages_are_gone_from_the_named_files() {
    // The flat strings WS06-S1 replaced must not come back beside the typed refusals.
    for (file, message) in [
        ("auth.rs", "ZDR credentials cannot access durable session writes"),
        ("auth.rs", "ZDR credentials cannot own durable runs"),
        ("memory_grpc.rs", "ZDR credentials cannot access durable memory"),
        ("memory_grpc.rs", "ZDR credentials cannot change memory controls"),
        ("message_search.rs", "recall is not available to zero-retention credentials"),
        ("terminalization.rs", "ZDR managed runs require a fresh metadata-only thread"),
    ] {
        let source = std::fs::read_to_string(src_path(file)).expect("a named site's file exists");
        let code = source
            .split("#[cfg(test)]")
            .next()
            .expect("split always yields a head");
        assert!(
            !code.contains(message),
            "{file} still carries the untyped refusal text {message:?}"
        );
    }
}
