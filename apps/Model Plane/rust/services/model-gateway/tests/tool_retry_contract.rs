//! Guards that the two tool-retry policies agree on their failure vocabulary.
//!
//! Retry lives in both dispatch loops and must stay in both — the loops are
//! separate on purpose (`CLAUDE.md`;
//! `docs/postmortem/0001-harn-1-2-tool-dispatch-unification.md`). What must
//! NOT differ is *which failures are considered transient*: if one loop retried
//! a class the other reported straight through, the same upstream blip would
//! be a recovered turn in chat and a hard failure in a deployed agent, and the
//! difference would be invisible until someone compared two incident reports.
//!
//! # Why this reads source text instead of importing
//!
//! Exactly the reasoning in `execution-core/tests/cross_service_loop_contract.rs`,
//! which does the same for the round-budget constants: the two services are
//! separate crates with separate deployment cadence and neither depends on the
//! other. A build dependency added purely so a test could compare two string
//! lists would couple their build graphs more heavily than the invariant it
//! protects. Both marker lists are also (correctly) private to their modules;
//! widening either to `pub` for test visibility would export dispatch internals
//! as API.
//!
//! If either file moves, this fails — the right outcome. Re-point the path; do
//! not delete the assertion.

use std::path::{Path, PathBuf};

/// `.../rust/services` — both service crates live side by side under it.
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
            "cannot read {} ({error}). If the file moved, re-point this test — \
             half the retry invariant moved with it.",
            path.display()
        )
    })
}

/// Extract the `&["a", "b", ...]` entries of a named `const NAME: &[&str]`.
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
    let mut chars = body.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        // Skip line comments wholesale so a `// "quoted"` aside is never
        // mistaken for an entry.
        if ch == '/' && body[index..].starts_with("//") {
            for (_, c) in chars.by_ref() {
                if c == '\n' {
                    break;
                }
            }
            continue;
        }
        if ch != '"' {
            continue;
        }
        let literal_start = index + 1;
        let mut literal_end = literal_start;
        for (inner_index, inner) in body[literal_start..].char_indices() {
            if inner == '"' {
                literal_end = literal_start + inner_index;
                break;
            }
        }
        out.push(body[literal_start..literal_end].to_owned());
        // Advance past the literal we just consumed.
        while let Some(&(next_index, _)) = chars.peek() {
            if next_index <= literal_end {
                chars.next();
            } else {
                break;
            }
        }
    }
    out
}

const GATEWAY_RETRY: &str = "model-gateway/src/tool_retry.rs";
const EXECUTION_RETRY: &str = "execution-core/src/runtime_loop/retry.rs";
const GATEWAY_TOOL_LOOP: &str = "model-gateway/src/tool_loop.rs";

#[test]
fn both_loops_classify_the_same_failures_as_transient() {
    let gateway = read_source(GATEWAY_RETRY);
    let execution = read_source(EXECUTION_RETRY);

    let gateway_markers = string_slice_const(&gateway, "TRANSIENT_MARKERS");
    let execution_markers = string_slice_const(&execution, "TRANSIENT_MARKERS");

    assert!(
        !gateway_markers.is_empty(),
        "parsed an empty TRANSIENT_MARKERS from the gateway — the parser broke, \
         not the invariant; a vacuous pass here would hide real drift"
    );
    assert_eq!(
        gateway_markers, execution_markers,
        "the two tool loops disagree on which failures are transient. Add the \
         marker to BOTH {GATEWAY_RETRY} and {EXECUTION_RETRY}, or neither."
    );
}

#[test]
fn both_loops_exclude_the_same_permanent_failures() {
    let gateway = read_source(GATEWAY_RETRY);
    let execution = read_source(EXECUTION_RETRY);

    let gateway_markers = string_slice_const(&gateway, "NOT_TRANSIENT_MARKERS");
    let execution_markers = string_slice_const(&execution, "NOT_TRANSIENT_MARKERS");

    assert!(
        !gateway_markers.is_empty(),
        "parsed an empty NOT_TRANSIENT_MARKERS from the gateway — parser broke"
    );
    assert_eq!(
        gateway_markers, execution_markers,
        "the two tool loops disagree on which failures are permanently \
         non-retryable. A class excluded on one loop and retried on the other \
         is the drift this test exists to catch."
    );
}

#[test]
fn both_loops_read_the_providers_token_ceiling_vocabulary_identically() {
    // `stop_reason` is the PROVIDER's vocabulary. If one loop treated
    // "length" as a ceiling hit and the other did not, the same truncated
    // round would refuse its half-written tool call on one surface and
    // dispatch it on the other — acting on arguments that are an accident of
    // where the ceiling fell. Both sides match on the same two literals.
    let gateway = read_source(GATEWAY_TOOL_LOOP);
    let execution = read_source(EXECUTION_RETRY);

    for (source, which) in [(&gateway, GATEWAY_TOOL_LOOP), (&execution, EXECUTION_RETRY)] {
        let start = source
            .find("fn output_hit_token_ceiling")
            .unwrap_or_else(|| panic!("output_hit_token_ceiling not found in {which}"));
        let body = &source[start..];
        let end = body.find('}').expect("function has no closing brace");
        let body = &body[..end];
        assert!(
            body.contains("\"max_tokens\"") && body.contains("\"length\""),
            "{which} no longer matches both provider spellings of the token \
             ceiling — the two loops would disagree about which rounds are \
             truncated"
        );
        assert!(
            body.contains("to_ascii_lowercase") && body.contains("trim"),
            "{which} must normalize case and whitespace; a provider sending \
             \"MAX_TOKENS \" would otherwise slip past on one loop only"
        );
    }
}

#[test]
fn both_loops_allow_the_same_number_of_attempts() {
    let gateway = read_source(GATEWAY_RETRY);
    let execution = read_source(EXECUTION_RETRY);

    let needle = "pub const MAX_TOOL_ATTEMPTS: u32 = ";
    let value_of = |source: &str, which: &str| -> u32 {
        let start = source
            .find(needle)
            .unwrap_or_else(|| panic!("MAX_TOOL_ATTEMPTS not found in {which}"))
            + needle.len();
        let rest = &source[start..];
        let end = rest.find(';').expect("MAX_TOOL_ATTEMPTS has no `;`");
        rest[..end]
            .trim()
            .parse()
            .expect("MAX_TOOL_ATTEMPTS is not an integer literal")
    };

    assert_eq!(
        value_of(&gateway, GATEWAY_RETRY),
        value_of(&execution, EXECUTION_RETRY),
        "one loop gives a flaky tool more chances than the other; the same \
         upstream blip would then succeed on one surface and fail on the other"
    );
}
