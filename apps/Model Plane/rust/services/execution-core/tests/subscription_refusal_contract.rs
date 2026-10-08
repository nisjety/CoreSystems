//! Pins execution-core's reader of ChatGPT-subscription refusals
//! (`src/subscription_refusal.rs`) to the services it has to agree with.
//!
//! inference-core declares a refusal on the failed `Infer` status's trailers;
//! execution-core reads it so a run pinned to the subscription can end with the
//! real reason; Session Core takes the receipt's failure code from a fixed
//! list. The three are deployed separately and none depends on another, so each
//! spells the trailer keys and reason codes as its own literals. A silent
//! disagreement would fail nothing: execution-core would simply stop
//! recognising the refusal and report the generic "route unavailable", hiding a
//! usage limit's reset time from the user again, or Session Core would reject
//! the receipt and leave the run open. This reads the sources, as
//! model-gateway's `subscription_failure_contract.rs` does for the gateway's
//! copy of the same reader.

use std::path::{Path, PathBuf};

fn services_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("execution-core's parent is rust/services")
        .to_path_buf()
}

fn read_source(relative: &str) -> String {
    let path = services_dir().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {} ({error}). If the file moved, re-point this test; do \
             not delete it — it is what keeps the services reading one refusal \
             vocabulary.",
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

/// The value of `const NAME: &str = "value";`, whatever its visibility.
fn str_const(source: &str, name: &str) -> String {
    let decl = format!("const {name}: &str = \"");
    let start = source
        .find(&decl)
        .unwrap_or_else(|| panic!("`{name}` not found — was it renamed?"))
        + decl.len();
    let end = source[start..].find('"').expect("a closing quote");
    source[start..start + end].to_owned()
}

const EXECUTION: &str = "execution-core/src/subscription_refusal.rs";
const INFERENCE_GRPC: &str = "inference-core/src/grpc.rs";
const INFERENCE_PROVIDER: &str = "inference-core/src/provider/mod.rs";
const INFERENCE_BROKER: &str = "inference-core/src/provider/codex_subscription.rs";
const SESSION_TERMINALIZATION: &str = "session-core/src/terminalization.rs";

/// The five reasons inference-core can declare, as execution-core names them.
const REASON_CONSTS: [&str; 5] = [
    "SUBSCRIPTION_USAGE_LIMIT",
    "SUBSCRIPTION_REAUTH_REQUIRED",
    "SUBSCRIPTION_UPSTREAM_UNAVAILABLE",
    "SUBSCRIPTION_BAD_REQUEST",
    "RATE_LIMITED",
];

#[test]
fn both_sides_spell_the_trailers_identically() {
    let execution = read_source(EXECUTION);
    let inference = read_source(INFERENCE_GRPC);
    for (name, spelling) in [
        ("PROVIDER_ERROR_KIND_TRAILER", "x-mp-provider-error"),
        ("RETRY_AFTER_MS_TRAILER", "x-mp-retry-after-ms"),
        ("RESETS_AT_TRAILER", "x-mp-resets-at"),
    ] {
        assert_eq!(str_const(&execution, name), spelling, "{EXECUTION} {name}");
        assert_eq!(
            str_const(&inference, name),
            spelling,
            "{INFERENCE_GRPC} {name}"
        );
    }
    // The overflow reason rides the same key under the value `too_long`.
    assert_eq!(
        str_const(&execution, "PROVIDER_ERROR_TOO_LONG"),
        str_const(&inference, "PROVIDER_ERROR_TOO_LONG"),
    );
}

#[test]
fn execution_core_reads_every_reason_inference_core_can_declare() {
    let declared = literals_in_fn(
        &read_source(INFERENCE_PROVIDER),
        "pub const fn code(self) -> &'static str",
    );
    assert_eq!(
        declared.len(),
        4,
        "parsed {declared:?} — the parser broke, or a reason was added; either \
         way execution-core must learn it before this passes"
    );
    let execution = read_source(EXECUTION);
    let read: Vec<String> = REASON_CONSTS
        .iter()
        .map(|name| str_const(&execution, name))
        .collect();
    for code in &declared {
        assert!(
            read.contains(code),
            "inference-core declares `{code}` and execution-core does not read it"
        );
    }
    // The throttle is typed by the trailer value, not by `code()`.
    assert_eq!(
        str_const(&read_source(INFERENCE_GRPC), "PROVIDER_ERROR_RATE_LIMITED"),
        str_const(&execution, "RATE_LIMITED"),
    );
}

/// inference-core types the broker's codes by these spellings; they must be the
/// same words execution-core then reads, so one vocabulary runs end to end.
#[test]
fn inference_core_reads_the_broker_by_the_same_codes() {
    let broker = read_source(INFERENCE_BROKER);
    let execution = read_source(EXECUTION);
    for (broker_name, execution_name) in [
        ("USAGE_LIMIT", "SUBSCRIPTION_USAGE_LIMIT"),
        ("REAUTH_REQUIRED", "SUBSCRIPTION_REAUTH_REQUIRED"),
        ("UPSTREAM_UNAVAILABLE", "SUBSCRIPTION_UPSTREAM_UNAVAILABLE"),
        ("BAD_REQUEST", "SUBSCRIPTION_BAD_REQUEST"),
        ("RATE_LIMITED", "RATE_LIMITED"),
    ] {
        assert_eq!(
            str_const(&broker, broker_name),
            str_const(&execution, execution_name),
            "{broker_name}"
        );
    }
}

/// The terminal receipt's failure code must be on Session Core's allowlist, or
/// Session Core rejects the receipt and the run is left open for the deadline
/// watchdog to force-fail.
#[test]
fn the_terminal_failure_codes_are_on_session_cores_allowlist() {
    let session = read_source(SESSION_TERMINALIZATION);
    let start = session
        .find("const ALLOWED_FAILURE_CODES")
        .expect("`ALLOWED_FAILURE_CODES` not found — was it renamed?");
    let list = &session[start..start + session[start..].find("];").expect("end of list")];
    let execution = read_source(EXECUTION);
    for name in ["TERMINAL_PROVIDER_UNAVAILABLE", "TERMINAL_INFERENCE_FAILED"] {
        let code = str_const(&execution, name);
        assert!(
            list.contains(&format!("\"{code}\"")),
            "execution-core records `{code}` ({name}) and Session Core does not allow it"
        );
    }
    // The generic code the loop has always recorded must stay allowed too.
    assert!(list.contains("\"execution_failed\""));
}
