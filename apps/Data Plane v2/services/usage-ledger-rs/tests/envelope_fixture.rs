//! The contract between this crate and its consumer, data-orchestrator (Go).
//!
//! `tests/fixtures/usage-envelopes.v2.json` holds real signed envelopes: each is built by
//! [`UsageBuilder`] from a provider answer, signed by the real [`EventSigner`] of the
//! producer that would emit it, and listed with the public key that verifies it and the
//! fields a consumer must read out of it. The Go verifier and consumer
//! (`services/data-orchestrator-go/internal/cost`) read a byte-for-byte copy of the file
//! (`testdata/usage-envelopes.v2.json`) through their real code, so a field renamed on one
//! side, a feature the verifier does not know or a scope that does not match fails a test
//! on the side that is wrong, instead of surfacing as an event the live consumer logs and
//! drops.
//!
//! Nothing in the file is typed by hand: it is generated.
//!
//! ```text
//! UPDATE_USAGE_FIXTURE=1 cargo test -p usage-ledger-rs --test envelope_fixture
//! ```
//!
//! The run signs with throwaway keys made on the spot (the private halves are never
//! written down), then writes the file and its sha256 pin. Copy both to
//! `services/data-orchestrator-go/testdata/` in the same change;
//! `scripts/check-usage-contract.sh` fails when the two copies differ. The envelopes are
//! signed at `signed_at`, and the Go tests read them at that moment: an envelope's lifetime
//! is two minutes, so a fixture read later is authentic but stale, which is exactly the
//! state the durable consumer is built to accept.

use std::path::PathBuf;

use event_envelope_rs::{EnvelopeError, EventSigner, EventVerifier};
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::pkcs8::{EncodePublicKey, LineEnding};
use rsa::{RsaPrivateKey, RsaPublicKey};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use usage_ledger_rs::{CallUsage, Feature, UsageBuilder, UsageEvent, WireTiming, WireUsage};

const FIXTURE: &str = "tests/fixtures/usage-envelopes.v2.json";
const PIN: &str = "tests/fixtures/usage-envelopes.v2.json.sha256";
const AUDIENCE: &str = "dataplane-events";

struct Producer {
    name: &'static str,
    issuer: &'static str,
    key_id: &'static str,
    scope: &'static str,
}

const PRODUCERS: [Producer; 3] = [
    Producer {
        name: "embedding-engine-rs",
        issuer: "service:embedding-engine-rs",
        key_id: "embedding-events-v1",
        scope: "events:embedding:publish",
    },
    Producer {
        name: "graph-index-rs",
        issuer: "service:graph-index-rs",
        key_id: "graph-events-v1",
        scope: "events:graph:publish",
    },
    Producer {
        name: "retrieval-engine-rs",
        issuer: "service:retrieval-engine-rs",
        key_id: "retrieval-events-v1",
        scope: "events:retrieval:publish",
    },
];

fn path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

/// The pin is the sha256 of the file with CRLF normalised to LF, as every vendored
/// contract of the Data Plane is pinned, so a checkout that rewrites line endings does not
/// move it.
fn pin_of(text: &str) -> String {
    let normalised = text.replace("\r\n", "\n");
    Sha256::digest(normalised.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn wire(
    request_id: &'static str,
    model: &'static str,
    input: i32,
    output: i32,
    cache: i32,
) -> CallUsage {
    CallUsage::from_wire(WireUsage {
        request_id,
        model_used: model,
        provider_used: "azure-openai",
        input_tokens: input,
        output_tokens: output,
        cache_read_input_tokens: cache,
        timing: Some(WireTiming {
            usage_reported: true,
            response_cache_hit: false,
        }),
    })
}

fn only(feature: Feature, org: &str, zdr: bool, calls: Vec<CallUsage>) -> UsageEvent {
    let mut builder = UsageBuilder::new(feature, org, zdr).expect("builder");
    for call in calls {
        builder.push(call);
    }
    let mut events = builder.build();
    assert_eq!(events.len(), 1, "a case is one event");
    events.remove(0)
}

fn expectation(event: &UsageEvent) -> Value {
    json!({
        "org_id": event.org_id,
        "zdr": event.zdr,
        "event_type": event.event_type,
        "feature": event.feature,
        "model_used": event.model_used,
        "provider_used": event.provider_used,
        "count": event.count,
        "input_tokens": event.input_tokens,
        "output_tokens": event.output_tokens,
        "cache_read_tokens": event.cache_read_tokens,
        "units": event.units,
        "cost_basis": event.cost_basis,
        "estimated": event.estimated,
        "request_digest": event.request_digest,
        "idempotency_key": event.idempotency_key,
    })
}

fn generate() -> Value {
    let signed_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs();

    let mut producers = serde_json::Map::new();
    let mut signers = Vec::new();
    for producer in &PRODUCERS {
        let private = RsaPrivateKey::new(&mut rand::thread_rng(), 2048).expect("test RSA key");
        let public = RsaPublicKey::from(&private)
            .to_public_key_pem(LineEnding::LF)
            .expect("public key pem");
        let signer = EventSigner::from_rsa_pem(
            private
                .to_pkcs1_pem(LineEnding::LF)
                .expect("private key pem")
                .as_bytes(),
            producer.issuer,
            producer.key_id,
            AUDIENCE,
            producer.scope,
        )
        .expect("signer");
        producers.insert(
            producer.name.to_owned(),
            json!({
                "issuer": producer.issuer,
                "key_id": producer.key_id,
                "scope": producer.scope,
                "public_key_pem": public,
            }),
        );
        signers.push(signer);
    }
    let signer_of = |name: &str| {
        let index = PRODUCERS
            .iter()
            .position(|producer| producer.name == name)
            .expect("producer");
        &signers[index]
    };

    let cases: Vec<(&str, &str, UsageEvent)> = vec![
        (
            "contextual-retrieval-exact",
            "embedding-engine-rs",
            // Three calls of one document: the numbers of a real gpt-4o-mini answer, the
            // second of which was partly served from the provider's prompt cache.
            only(
                Feature::ContextualRetrieval,
                "org-fixture-a",
                false,
                vec![
                    wire("req-ctx-1", "gpt-4o-mini-2024-07-18", 1218, 61, 0),
                    wire("req-ctx-2", "gpt-4o-mini-2024-07-18", 1230, 57, 1024),
                    wire("req-ctx-3", "gpt-4o-mini-2024-07-18", 1224, 59, 1024),
                ],
            ),
        ),
        (
            "embedding-estimated",
            "embedding-engine-rs",
            only(
                Feature::Embedding,
                "org-fixture-a",
                false,
                vec![CallUsage::estimated(
                    "call-embed-1",
                    "embed-v-4-0",
                    "azure-cohere",
                    24,
                    9100,
                )],
            ),
        ),
        (
            "contextual-retrieval-zero-retention",
            "embedding-engine-rs",
            only(
                Feature::ContextualRetrieval,
                "org-fixture-zdr",
                true,
                vec![wire("req-zdr-1", "gpt-4o-mini-2024-07-18", 900, 40, 0)],
            ),
        ),
        (
            "graph-extraction",
            "graph-index-rs",
            only(
                Feature::GraphExtraction,
                "org-fixture-a",
                false,
                vec![
                    wire("req-gx-1", "gpt-4o", 2100, 480, 0),
                    wire("req-gx-2", "gpt-4o", 1900, 300, 0),
                ],
            ),
        ),
        (
            "graph-community-summary-zero-retention",
            "graph-index-rs",
            only(
                Feature::GraphCommunitySummary,
                "org-fixture-zdr",
                true,
                vec![wire("req-gs-1", "gpt-4o", 3000, 220, 0)],
            ),
        ),
        (
            "web-rerank-units",
            "retrieval-engine-rs",
            only(
                Feature::WebRerank,
                "org-fixture-a",
                false,
                vec![wire("req-wr-1", "rerank-v4.0-pro", 0, 0, 0).with_units(3)],
            ),
        ),
    ];

    let cases: Vec<Value> = cases
        .into_iter()
        .map(|(name, producer, event)| {
            let envelope = event.sign(signer_of(producer)).expect("signed event");
            json!({
                "name": name,
                "producer": producer,
                "envelope": String::from_utf8(envelope).expect("an envelope is JSON text"),
                "expect": expectation(&event),
            })
        })
        .collect();

    json!({
        "contract": "usage-ledger-rs envelopes, schema_version 2",
        "generator": "UPDATE_USAGE_FIXTURE=1 cargo test -p usage-ledger-rs --test envelope_fixture",
        "audience": AUDIENCE,
        "subject": usage_ledger_rs::SUBJECT_COST_LEDGER,
        "signed_at": signed_at,
        "producers": producers,
        "cases": cases,
    })
}

fn write_fixture() {
    let mut text = serde_json::to_string_pretty(&generate()).expect("fixture json");
    text.push('\n');
    std::fs::write(path(FIXTURE), &text).expect("write the fixture");
    std::fs::write(path(PIN), format!("{}\n", pin_of(&text))).expect("write the pin");
    eprintln!(
        "usage envelope fixture regenerated: copy it and its pin to data-orchestrator-go/testdata/"
    );
}

/// Regenerate once per run, before any test reads the file, when asked to.
fn ensure_generated() {
    static GENERATE: std::sync::Once = std::sync::Once::new();
    GENERATE.call_once(|| {
        if std::env::var_os("UPDATE_USAGE_FIXTURE").is_some() {
            write_fixture();
        }
    });
}

fn committed() -> (String, Value) {
    ensure_generated();
    let text = std::fs::read_to_string(path(FIXTURE)).unwrap_or_else(|error| {
        panic!(
            "{FIXTURE} is missing ({error}). Generate it: UPDATE_USAGE_FIXTURE=1 cargo test -p \
             usage-ledger-rs --test envelope_fixture. A missing contract is a failure, never a skip"
        )
    });
    let value = serde_json::from_str(&text).expect("the fixture is JSON");
    (text, value)
}

#[test]
fn the_committed_fixture_matches_its_pin_and_is_what_this_code_still_produces() {
    let (text, fixture) = committed();
    let pin = std::fs::read_to_string(path(PIN)).expect("the pin file");
    assert_eq!(
        pin.trim(),
        pin_of(&text),
        "the fixture was edited without regenerating its pin"
    );

    // Every envelope is authentic: verified with the public key the fixture lists, under the
    // issuer, key id and scope of its producer. Stale (the lifetime is two minutes) is
    // expected and is still authentic; anything else is a defect in the fixture.
    let signed_at = fixture["signed_at"].as_u64().expect("signed_at");
    assert!(signed_at > 1_700_000_000);
    assert_eq!(fixture["audience"], AUDIENCE);
    assert_eq!(fixture["subject"], "dataplane.cost.ledger");
    let cases = fixture["cases"].as_array().expect("cases");
    assert!(cases.len() >= 6);
    for case in cases {
        let producer = case["producer"].as_str().expect("producer");
        let entry = &fixture["producers"][producer];
        let verifier = EventVerifier::from_rsa_pem(
            entry["public_key_pem"].as_str().expect("key").as_bytes(),
            entry["issuer"].as_str().expect("issuer"),
            entry["key_id"].as_str().expect("kid"),
            AUDIENCE,
            entry["scope"].as_str().expect("scope"),
            100,
        )
        .expect("verifier");
        let envelope = case["envelope"].as_str().expect("envelope").as_bytes();
        let (claims, payload) = match verifier.verify("dataplane.cost.ledger", envelope) {
            Ok(verified) => (verified.claims, verified.payload),
            Err(EnvelopeError::Expired(verified)) => (verified.claims, verified.payload),
            Err(error) => panic!("case {}: {error}", case["name"]),
        };
        let decoded: Value = serde_json::from_slice(&payload).expect("payload json");
        let expect = &case["expect"];
        assert_eq!(claims.org_id, expect["org_id"].as_str().unwrap());
        assert_eq!(claims.zdr, expect["zdr"].as_bool().unwrap());
        assert_eq!(claims.user_id, None, "a usage event names no user");
        assert_eq!(decoded["schema_version"], 2);
        for key in [
            "org_id",
            "zdr",
            "event_type",
            "feature",
            "model_used",
            "provider_used",
            "count",
            "input_tokens",
            "output_tokens",
            "cache_read_tokens",
            "cost_basis",
            "estimated",
            "request_digest",
            "idempotency_key",
        ] {
            assert_eq!(
                decoded[key], expect[key],
                "case {} field {key}",
                case["name"]
            );
        }
        assert_eq!(
            decoded.get("units").cloned().unwrap_or(Value::Null),
            expect["units"],
            "case {} field units",
            case["name"]
        );
    }
}

#[test]
fn the_cases_cover_what_the_consumer_must_decide() {
    let (_, fixture) = committed();
    let names: Vec<&str> = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| case["name"].as_str().unwrap())
        .collect();
    for needed in [
        "contextual-retrieval-exact",
        "embedding-estimated",
        "contextual-retrieval-zero-retention",
        "graph-extraction",
        "graph-community-summary-zero-retention",
        "web-rerank-units",
    ] {
        assert!(
            names.contains(&needed),
            "the fixture lost the case {needed}"
        );
    }
    // Both postures and all three producers are in it, so a verifier that pins the wrong key
    // or drops zero-retention events fails on a case, not on a guess.
    let flags: Vec<bool> = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| case["expect"]["zdr"].as_bool().unwrap())
        .collect();
    assert!(flags.contains(&true) && flags.contains(&false));
    for producer in PRODUCERS.iter().map(|producer| producer.name) {
        assert!(
            fixture["cases"]
                .as_array()
                .unwrap()
                .iter()
                .any(|case| case["producer"] == producer),
            "no case for {producer}"
        );
    }
}
