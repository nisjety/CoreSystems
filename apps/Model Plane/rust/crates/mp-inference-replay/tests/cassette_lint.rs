//! Keeps the shipped cassettes synthetic and not ZDR, and keeps the replay fake out of every
//! service's runtime dependencies.

use std::fs;
use std::path::Path;

use mp_inference_replay::{cassette_dir, Cassette};

#[test]
fn cassettes_are_synthetic_and_not_zdr() {
    let mut seen = Vec::new();
    for entry in fs::read_dir(cassette_dir()).expect("the cassette directory exists") {
        let path = entry.expect("a directory entry").path();
        if path
            .extension()
            .is_some_and(|extension| extension == "jsonl")
        {
            let source = fs::read_to_string(&path).expect("the cassette is readable");
            let cassette = Cassette::parse(&source)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert!(
                cassette.header.synthetic,
                "{} is not synthetic",
                path.display()
            );
            assert!(!cassette.header.zdr, "{} is marked zdr", path.display());
            seen.push(cassette.header.id);
        }
    }
    seen.sort();
    assert_eq!(
        seen,
        [
            "01-plain-answer",
            "04-honest-refusal",
            "07-approval-pause",
            "10-two-tool-loop",
            "13-compaction-survivor",
            "15-do-run-with-plan",
        ],
        "the six sessions of the design, and no others"
    );
}

#[test]
fn replay_crate_not_a_runtime_dependency() {
    let services = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../services");
    let mut checked = 0;
    for entry in fs::read_dir(&services).expect("the services directory exists") {
        let manifest_path = entry.expect("a directory entry").path().join("Cargo.toml");
        if let Ok(manifest) = fs::read_to_string(&manifest_path) {
            assert!(
                !lists_as_runtime_dependency(&manifest, "mp-inference-replay"),
                "{} lists the replay fake as a runtime dependency",
                manifest_path.display()
            );
            checked += 1;
        }
    }
    assert!(checked >= 4, "the services were scanned; found {checked}");
}

#[test]
fn dependency_scan_reads_only_runtime_tables() {
    let dev_only =
        "[dependencies]\nfoo = \"1\"\n[dev-dependencies]\nmp-inference-replay = { path = \"x\" }\n";
    assert!(!lists_as_runtime_dependency(
        dev_only,
        "mp-inference-replay"
    ));
    let plain = "[dependencies]\nmp-inference-replay = { path = \"x\" }\n";
    assert!(lists_as_runtime_dependency(plain, "mp-inference-replay"));
    let target = "[target.'cfg(unix)'.dependencies]\nmp-inference-replay = { path = \"x\" }\n";
    assert!(lists_as_runtime_dependency(target, "mp-inference-replay"));
    let inline = "[dependencies.mp-inference-replay]\npath = \"x\"\n";
    assert!(lists_as_runtime_dependency(inline, "mp-inference-replay"));
}

/// Whether `manifest` lists `name` under a table a binary links: `[dependencies]`,
/// `[build-dependencies]` or a `[target.*.dependencies]` table. `[dev-dependencies]` is where the
/// replay fake belongs, so it is not a match.
fn lists_as_runtime_dependency(manifest: &str, name: &str) -> bool {
    let inline_table = format!("dependencies.{name}");
    let mut in_runtime_table = false;
    for line in manifest.lines().map(str::trim) {
        if let Some(table) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            if table == inline_table {
                return true;
            }
            in_runtime_table = table == "dependencies"
                || table == "build-dependencies"
                || (table.starts_with("target.") && table.ends_with(".dependencies"));
            continue;
        }
        if in_runtime_table && line.split('=').next().map(str::trim) == Some(name) {
            return true;
        }
    }
    false
}
