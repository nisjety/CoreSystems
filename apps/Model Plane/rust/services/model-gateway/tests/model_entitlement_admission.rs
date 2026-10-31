//! Gateway admission under the organisation's model policy (ADR-0005, WS10-S4).
//!
//! The transport behaviour (typed 403, one error frame, zero session-core rows)
//! is proved against a mock inference-core in `e2e_invoke_chain_test.rs` and in
//! the gRPC module of `src/grpc.rs`. This file pins the two things those cannot
//! see from outside: where admission sits on each transport (before the first
//! durable call, so a refusal leaves nothing behind) and that every place that
//! turns an inference-core status into an answer reads the entitlement trailers
//! before it falls through to a generic failure. It also exercises the public
//! effort clamp.

use std::path::Path;

use model_gateway::model_access::{clamp_profile, Admitted};

fn source(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {} ({error})", path.display()))
        .replace("\r\n", "\n")
}

/// The text of the item that starts at `signature`, up to the next top-level
/// (or, for a method, next 4-space-indented) item boundary.
fn item(source: &str, signature: &str, end: &str) -> String {
    let start = source
        .find(signature)
        .unwrap_or_else(|| panic!("`{signature}` not found; was it renamed?"));
    let rest = &source[start..];
    let stop = rest[signature.len()..]
        .find(end)
        .map_or(rest.len(), |offset| offset + signature.len());
    rest[..stop].to_owned()
}

/// Every durable call of a turn that admission has to precede.
const DURABLE: [&str; 3] = [
    "prepare_managed_run",
    "idempotency\n            .claim(",
    "create_thread",
];

fn assert_admission_first(label: &str, body: &str) {
    let admit = body
        .find("model_access::admit(")
        .unwrap_or_else(|| panic!("{label} does not ask for admission"));
    for durable in DURABLE {
        if let Some(at) = body.find(durable) {
            assert!(
                admit < at,
                "{label}: admission must come before `{durable}`, or a refused turn leaves rows"
            );
        }
    }
    assert_eq!(
        body.matches("model_access::admit(").count(),
        1,
        "{label}: one question per turn"
    );
}

#[test]
fn admission_is_asked_before_any_durable_call_on_every_invoke_transport() {
    let http = source("src/http_routes.rs");
    assert_admission_first(
        "unary /v1/invoke",
        &item(&http, "async fn invoke(\n", "\n}\n"),
    );

    let sse = source("src/sse.rs");
    assert_admission_first(
        "streaming /v1/invoke/stream",
        &item(&sse, "pub async fn invoke_stream_sse(", "\n}\n"),
    );

    let grpc = source("src/grpc.rs");
    for method in ["    async fn invoke(\n", "    async fn invoke_stream(\n"] {
        assert_admission_first(
            &format!("gRPC {}", method.trim()),
            &item(&grpc, method, "\n    }\n"),
        );
    }
}

#[test]
fn a_refusal_before_the_run_ends_the_turn_the_way_each_transport_ends_one() {
    let sse = source("src/sse.rs");
    let invoke = item(&sse, "pub async fn invoke_stream_sse(", "\n}\n");
    assert!(
        invoke.contains("refusal.code(),") && invoke.contains("refusal.retryable(),"),
        "the stream's refusal is its `error` frame with the catalogue code and retry flag"
    );

    let http = source("src/http_routes.rs");
    let unary = item(&http, "async fn invoke(\n", "\n}\n");
    assert!(unary.contains("model_access::http(&refusal)"));

    let grpc = source("src/grpc.rs");
    assert!(
        grpc.matches(".map_err(|refusal| refusal.grpc_status())?").count() >= 2,
        "both gRPC invoke methods refuse with the catalogue status"
    );
}

#[test]
fn every_place_that_maps_an_inference_status_reads_the_entitlement_trailers() {
    // The new refusals travel on PERMISSION_DENIED and UNAVAILABLE, which the
    // generic mappings turn into a flat 403 and a 502. Each place below reads the
    // kind trailer first, so the catalogue code survives.
    let http = source("src/http_routes.rs");
    for (label, signature) in [
        (
            "grpc_status_to_http",
            "pub(crate) fn grpc_status_to_http(",
        ),
        ("invoke_inference_error", "fn invoke_inference_error("),
    ] {
        let body = item(&http, signature, "\n}\n");
        assert!(
            body.contains("model_entitlement::Refusal::from_status"),
            "{label} must keep the entitlement code"
        );
    }

    let sse = source("src/sse.rs");
    let zdr = item(&sse, "async fn zdr_direct_stream(", "\n}\n");
    let typed = zdr
        .find("model_entitlement::Refusal::from_status")
        .expect("the persistence-free stream reads the entitlement trailers");
    let generic = zdr
        .find("ephemeral inference failed")
        .expect("the generic failure arm exists");
    assert!(typed < generic, "typed before generic");
    assert!(
        sse.contains("model_entitlement::Refusal::from_status(&e)"),
        "the unary fallback of the durable stream reads them too"
    );
}

#[test]
fn a_surface_with_no_policy_reaches_the_turn_byte_for_byte() {
    let none = Admitted::default();
    assert_eq!(none.model, None);
    assert!(none.max_effort.is_empty());
    for effort in [None, Some("deep"), Some("max"), Some("standard"), Some("wat")] {
        for ladder in [false, true] {
            let mut requested = effort.map(str::to_owned);
            none.clamp_effort(&mut requested, ladder);
            assert_eq!(requested.as_deref(), effort, "{effort:?} ladder={ladder}");
        }
    }
}

#[test]
fn the_effort_clamp_only_ever_lowers_a_request() {
    let names = [
        "minimal", "low", "medium", "high", "xhigh", "max", "quick", "deep", "standard",
    ];
    let rank = |name: &str| match name {
        "minimal" => 1,
        "low" | "quick" => 2,
        "medium" => 3,
        "high" | "deep" => 4,
        "xhigh" => 5,
        "max" => 6,
        _ => 0,
    };
    for ladder in [true, false] {
        for asked in names {
            for ceiling in ["minimal", "low", "medium", "high", "xhigh", "max"] {
                let Some(clamped) = clamp_profile(asked, ceiling, ladder) else {
                    continue;
                };
                assert!(
                    rank(&clamped) < rank(asked),
                    "{asked} under {ceiling} (ladder={ladder}) became {clamped}, which is not lower"
                );
                assert!(
                    rank(&clamped) <= rank(ceiling),
                    "{asked} under {ceiling} (ladder={ladder}) became {clamped}, above the ceiling"
                );
            }
        }
    }
}
