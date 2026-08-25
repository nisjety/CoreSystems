//! Pins the gateway's SSE relay as **byte-verbatim**: it must never inspect,
//! filter, or allowlist upstream event names.
//!
//! This is the §0 root-cause invariant from `model-plane-to-verevon-parity.md`.
//! The Model Plane adds chat events regularly (`memory_recall` and
//! `reasoning_delta` most recently), and the whole reason the SPA can pick them
//! up with a frontend-only change is that nothing in between decides which
//! events are allowed through. The moment the relay grows a `match` on event
//! names, every new backend event needs a gateway release too — and worse, the
//! failure is silent: the stream still succeeds, the new feature simply never
//! appears, and the obvious place to look (the backend, which is emitting
//! correctly) is the wrong one.
//!
//! The relay is `upstream::proxy_sse_stream_with_data_plane`; the other
//! `proxy_sse_stream*` entry points delegate to it.
//!
//! # What this test does and does not prove
//!
//! Source-scanning, in the same spirit and style as
//! `tests/ssrf_forward_not_fetch.rs`: it reads the relay's own text and checks
//! that upstream bytes are yielded through untouched and that no chat event name
//! is mentioned anywhere in the file. It is a cheap tripwire, not a data-flow
//! prover — a sufficiently indirect filter could dodge it. That is the accepted
//! tradeoff; the realistic regression is someone adding a readable `match
//! event_name { ... }`, and this catches that.
//!
//! It derives the event names from **model-gateway's own `ChatEvent` enum**
//! rather than a hardcoded list, so a newly added backend event is covered by
//! this assertion automatically, with no second place to remember to update.
//!
//! # If this test fails
//!
//! Do not loosen it. Either the relay genuinely started inspecting event names —
//! which is the regression — or the relay was restructured and the scan needs
//! re-pointing. In the second case, say so in a comment here.

use std::fs;
use std::path::{Path, PathBuf};

/// `apps/gateway` → the verevonv3 app root → `apps/` → the Model Plane's
/// `ChatEvent` definition. Long, and deliberately hard-failing if it moves: a
/// silent skip would make this assertion vacuous exactly when it is needed.
fn chat_event_source() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../Model Plane/rust/services/model-gateway/src/sse_events.rs")
}

fn relay_source() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/upstream.rs");
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {} ({error})", path.display()))
}

/// The RELAY FUNCTION's body only.
///
/// Scoping matters: the first draft of this test scanned the whole file and
/// failed on `"error"`, which appears there twice as a JSON *envelope key* (an
/// error response is parsed, not an event filtered) and twice inside the test
/// module. Both are unrelated to the invariant, and widening the allowance to
/// let `"error"` through everywhere would have blinded the test to a real
/// allowlist that happened to include it. Narrowing the scan instead keeps the
/// assertion strict where it matters.
fn relay_function_body() -> String {
    let source = relay_source();
    const RELAY: &str = "pub(crate) async fn proxy_sse_stream_with_data_plane(";
    let start = source.find(RELAY).unwrap_or_else(|| {
        panic!(
            "{RELAY} not found in src/upstream.rs. The other proxy_sse_stream* \
             entry points delegate to it; if the relay was renamed or split, \
             re-point this test rather than deleting it."
        )
    });
    let open = source[start..]
        .find('{')
        .expect("relay function has no body")
        + start;
    let mut depth = 0usize;
    for (offset, ch) in source[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return source[open..=open + offset].to_owned();
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced braces in the relay function");
}

/// Event names from `ChatEvent::name()`'s match arms.
fn upstream_event_names() -> Vec<String> {
    let path = chat_event_source();
    let source = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read the Model Plane's ChatEvent definition at {} ({error}).\n\
             This test derives its event list from there on purpose, so a new \
             backend event is covered automatically. If the file moved, re-point \
             this path — do not delete the assertion.",
            path.display()
        )
    });
    let start = source
        .find("pub fn name(&self) -> &'static str")
        .expect("ChatEvent::name not found — was it renamed?");
    let body = &source[start..];
    let end = body.find("\n    }").expect("name() has no closing brace");
    let body = &body[..end];

    let mut names = Vec::new();
    for line in body.lines() {
        let Some(arrow) = line.find("=>") else { continue };
        let rest = &line[arrow..];
        let Some(open) = rest.find('"') else { continue };
        let after = &rest[open + 1..];
        if let Some(close) = after.find('"') {
            names.push(after[..close].to_owned());
        }
    }
    names
}

#[test]
fn the_relay_forwards_upstream_bytes_untouched() {
    let source = relay_function_body();
    assert!(
        source.contains("let upstream = resp.bytes_stream();"),
        "the relay no longer takes the upstream body as a raw byte stream; if it \
         now parses SSE frames, the no-allowlist property cannot hold"
    );
    assert!(
        source.contains("Some(Ok(bytes)) => yield Ok::<Bytes, std::io::Error>(bytes),"),
        "upstream chunks are no longer yielded through unmodified. Anything that \
         rewrites or filters them here decides which backend events the SPA is \
         allowed to see."
    );
}

#[test]
fn the_relay_never_mentions_a_chat_event_name() {
    let source = relay_function_body();
    let names = upstream_event_names();
    assert!(
        names.len() >= 5,
        "parsed only {} event names from ChatEvent — the parser broke, not the \
         invariant; a vacuous pass here defeats the whole test",
        names.len()
    );

    let mentioned: Vec<&String> = names
        .iter()
        .filter(|name| source.contains(&format!("\"{name}\"")))
        .collect();
    assert!(
        mentioned.is_empty(),
        "the SSE relay mentions these chat event names: {mentioned:?}\n\
         An allowlist (or any per-event special-casing) means every new Model \
         Plane event needs a gateway release, and until it ships the event is \
         dropped silently — the stream still succeeds and the feature just never \
         appears. Relay bytes verbatim instead."
    );
}

/// The one event the relay is allowed to synthesize is its own transport error,
/// and it must stay a *fallback* on a broken upstream — not a rewrite of
/// upstream frames.
#[test]
fn the_only_synthesized_event_is_the_transport_error() {
    let source = relay_function_body();
    assert!(
        source.contains("sse_transport_error_event()"),
        "the relay must still emit an honest transport-error event when the \
         upstream body breaks mid-stream; silence there reads to the client as a \
         completed answer"
    );
    assert!(
        source.contains("Some(Err(_)) => {"),
        "the transport-error event must be reached from the upstream error arm, \
         not from inspecting event content"
    );
}
