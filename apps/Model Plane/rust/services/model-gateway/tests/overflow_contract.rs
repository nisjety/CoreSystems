//! Guards that the gateway's overflow *fallback* still covers what
//! inference-core's authoritative classifier knows, and that the two agree on
//! the trailer that carries the typed signal.
//!
//! # Why a fallback needs pinning at all
//!
//! inference-core classifies overflow at the only layer that sees the raw
//! provider body and declares it on a gRPC trailer. The gateway prefers that
//! signal. But a rolling deploy pairs a NEW gateway with an OLD inference-core
//! that sends no trailer, and in that window the gateway's own table is the only
//! thing standing between a too-long prompt and a hard error the user sees.
//!
//! So the invariant is one-directional and deliberately so: the fallback must
//! recognise everything the authoritative table does. The reverse is fine — the
//! gateway may know an extra legacy wording — which is why this asserts a
//! superset, not equality.
//!
//! Reads source text for the same reason `tool_retry_contract.rs` does: two
//! separately deployed crates, neither depending on the other, and both tables
//! correctly private to their modules.

use std::path::{Path, PathBuf};

fn services_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("model-gateway's parent is rust/services")
        .to_path_buf()
}

fn read_source(relative: &str) -> String {
    let path = services_dir().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {} ({error}). If the file moved, re-point this test; do \
             not delete it — it is what keeps a rolling deploy from losing \
             overflow recovery.",
            path.display()
        )
    })
}

/// Extract the entries of a named `const NAME: &[&str]`, skipping comments.
fn string_slice_const(source: &str, name: &str) -> Vec<String> {
    let decl = format!("const {name}: &[&str] = &[");
    let start = source
        .find(&decl)
        .unwrap_or_else(|| panic!("`{name}` not found — was it renamed?"))
        + decl.len();
    let rest = &source[start..];
    let end = rest
        .find("];")
        .unwrap_or_else(|| panic!("`{name}` has no closing `];`"));
    let body = &rest[..end];

    let mut out = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if line.starts_with("//") {
            continue;
        }
        let mut chars = line.char_indices();
        while let Some((index, ch)) = chars.next() {
            if ch != '"' {
                continue;
            }
            let literal_start = index + 1;
            if let Some(offset) = line[literal_start..].find('"') {
                out.push(line[literal_start..literal_start + offset].to_owned());
                // Skip past the closing quote.
                for (next, _) in chars.by_ref() {
                    if next >= literal_start + offset {
                        break;
                    }
                }
            }
        }
    }
    out
}

const GATEWAY_COMPACTION: &str = "model-gateway/src/compaction.rs";
const INFERENCE_OVERFLOW: &str = "inference-core/src/provider/overflow.rs";
const INFERENCE_GRPC: &str = "inference-core/src/grpc.rs";

#[test]
fn the_gateway_fallback_covers_every_authoritative_length_marker() {
    let gateway = string_slice_const(&read_source(GATEWAY_COMPACTION), "LENGTH_MARKERS");
    let authoritative = string_slice_const(&read_source(INFERENCE_OVERFLOW), "LENGTH_MARKERS");

    assert!(
        !authoritative.is_empty(),
        "parsed an empty authoritative table — the parser broke, not the \
         invariant; a vacuous pass here would hide real drift"
    );
    let missing: Vec<&String> = authoritative
        .iter()
        .filter(|marker| !gateway.contains(marker))
        .collect();
    assert!(
        missing.is_empty(),
        "inference-core recognises these overflow wordings and the gateway \
         fallback does not: {missing:?}\nDuring a rolling deploy (new gateway, \
         old inference-core sending no trailer) each of these becomes a hard \
         error instead of a shed-and-retry. Add them to LENGTH_MARKERS in \
         {GATEWAY_COMPACTION}."
    );
}

#[test]
fn both_sides_exclude_every_authoritative_non_length_wording() {
    let gateway = string_slice_const(&read_source(GATEWAY_COMPACTION), "NOT_LENGTH_MARKERS");
    let authoritative = string_slice_const(&read_source(INFERENCE_OVERFLOW), "NOT_LENGTH_MARKERS");

    assert!(!authoritative.is_empty(), "parser broke, not the invariant");
    let missing: Vec<&String> = authoritative
        .iter()
        .filter(|marker| !gateway.contains(marker))
        .collect();
    assert!(
        missing.is_empty(),
        "inference-core excludes these as NOT overflow and the gateway does \
         not: {missing:?}\nThe gateway would shed conversation history in \
         response to a throttling or billing error, hiding the real failure \
         behind a truncated prompt."
    );
}

/// The typed signal is a string literal on both sides (separately deployed
/// crates, no dependency either way). A silent disagreement here would make the
/// gateway fall back to prose matching forever while looking type-first.
#[test]
fn both_sides_spell_the_typed_trailer_identically() {
    let gateway = read_source(GATEWAY_COMPACTION);
    let inference = read_source(INFERENCE_GRPC);

    for (needle, what) in [
        ("\"x-mp-provider-error\"", "trailer key"),
        ("\"too_long\"", "trailer value"),
    ] {
        assert!(
            gateway.contains(needle),
            "{GATEWAY_COMPACTION} no longer carries the {what} {needle}"
        );
        assert!(
            inference.contains(needle),
            "{INFERENCE_GRPC} no longer carries the {what} {needle}"
        );
    }
}
