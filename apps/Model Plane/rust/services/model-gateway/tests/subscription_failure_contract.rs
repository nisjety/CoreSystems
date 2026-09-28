//! Pins the ChatGPT-subscription refusal vocabulary between inference-core,
//! which declares a refusal on gRPC trailers, and the gateway, which reads it.
//!
//! The two crates are deployed separately and neither depends on the other,
//! so each spells the trailer keys and reason codes as its own literals. A
//! silent disagreement would not fail anything: the gateway would simply
//! stop recognising the refusal and fall back to its generic "the
//! subscription could not complete the answer", hiding a usage limit's reset
//! time from the user again. This reads both crates' source, as
//! `overflow_contract.rs` does for the overflow trailer.

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
             not delete it — it is what keeps the two crates reading one \
             refusal vocabulary.",
            path.display()
        )
    })
}

/// The string literals inside the body of the function whose signature
/// starts with `signature`, brace-matched.
fn literals_in_fn(source: &str, signature: &str) -> Vec<String> {
    let start = source
        .find(signature)
        .unwrap_or_else(|| panic!("`{signature}` not found — was it renamed?"));
    let open = start + source[start..].find('{').expect("a body");
    let mut depth = 0usize;
    let mut end = open;
    for (offset, ch) in source[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = open + offset;
                    break;
                }
            }
            _ => {}
        }
    }
    source[open..end]
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// The value of `const NAME: &str = "value";`.
fn str_const(source: &str, name: &str) -> String {
    let decl = format!("const {name}: &str = \"");
    let start = source
        .find(&decl)
        .unwrap_or_else(|| panic!("`{name}` not found — was it renamed?"))
        + decl.len();
    let end = source[start..].find('"').expect("a closing quote");
    source[start..start + end].to_owned()
}

const GATEWAY: &str = "model-gateway/src/subscription_failure.rs";
const GATEWAY_COMPACTION: &str = "model-gateway/src/compaction.rs";
const INFERENCE_GRPC: &str = "inference-core/src/grpc.rs";
const INFERENCE_PROVIDER: &str = "inference-core/src/provider/mod.rs";
const INFERENCE_BROKER: &str = "inference-core/src/provider/codex_subscription.rs";

#[test]
fn both_sides_spell_the_timing_trailers_identically() {
    let gateway = read_source(GATEWAY);
    let inference = read_source(INFERENCE_GRPC);
    for (name, spelling) in [
        ("RETRY_AFTER_MS_TRAILER", "x-mp-retry-after-ms"),
        ("RESETS_AT_TRAILER", "x-mp-resets-at"),
    ] {
        assert_eq!(str_const(&gateway, name), spelling, "{GATEWAY} {name}");
        assert_eq!(
            str_const(&inference, name),
            spelling,
            "{INFERENCE_GRPC} {name}"
        );
    }
    // The reason itself rides the overflow trailer's key.
    assert_eq!(
        str_const(
            &read_source(GATEWAY_COMPACTION),
            "PROVIDER_ERROR_KIND_TRAILER"
        ),
        str_const(&inference, "PROVIDER_ERROR_KIND_TRAILER"),
    );
}

#[test]
fn the_gateway_reads_every_reason_inference_core_can_declare() {
    let declared = literals_in_fn(
        &read_source(INFERENCE_PROVIDER),
        "pub const fn code(self) -> &'static str",
    );
    assert_eq!(
        declared.len(),
        4,
        "parsed {declared:?} — the parser broke, or a reason was added; either \
         way the gateway must learn it before this passes"
    );
    let gateway = read_source(GATEWAY);
    let gateway_values: Vec<String> = [
        "SUBSCRIPTION_USAGE_LIMIT",
        "SUBSCRIPTION_REAUTH_REQUIRED",
        "SUBSCRIPTION_UPSTREAM_UNAVAILABLE",
        "SUBSCRIPTION_BAD_REQUEST",
        "RATE_LIMITED",
    ]
    .iter()
    .map(|name| str_const(&gateway, name))
    .collect();
    for code in &declared {
        assert!(
            gateway_values.contains(code),
            "inference-core declares `{code}` and the gateway does not read it"
        );
    }
    assert_eq!(
        str_const(&read_source(INFERENCE_GRPC), "PROVIDER_ERROR_RATE_LIMITED"),
        str_const(&gateway, "RATE_LIMITED"),
    );
}

/// inference-core types the broker's codes by these spellings; they must be
/// the same words the gateway then reads, so one vocabulary runs end to end.
#[test]
fn inference_core_reads_the_broker_by_the_same_codes() {
    let broker = read_source(INFERENCE_BROKER);
    let gateway = read_source(GATEWAY);
    for (broker_name, gateway_name) in [
        ("USAGE_LIMIT", "SUBSCRIPTION_USAGE_LIMIT"),
        ("REAUTH_REQUIRED", "SUBSCRIPTION_REAUTH_REQUIRED"),
        ("UPSTREAM_UNAVAILABLE", "SUBSCRIPTION_UPSTREAM_UNAVAILABLE"),
        ("BAD_REQUEST", "SUBSCRIPTION_BAD_REQUEST"),
        ("RATE_LIMITED", "RATE_LIMITED"),
    ] {
        assert_eq!(
            str_const(&broker, broker_name),
            str_const(&gateway, gateway_name),
            "{broker_name}"
        );
    }
}
