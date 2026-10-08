//! The shared connection ZDR capability contract, checked against the real providers (Z-10).
//!
//! `connection-zdr-capabilities.json` (canonical copy in `apps/Model Plane/api/contracts/`) is
//! the one place that says whether a user-owned model connection can give zero data retention.
//! integration-corev2 embeds it (Go `go:embed`) and Auth Core only compares the opaque digest it
//! gets back. Go cannot link Rust, so the agreement between that table and what the providers
//! really report lives here: this test reads the vendored copy and asks the REAL
//! `CodexSubscriptionProvider` what it supports and how it answers a ZDR request, never a
//! hand-copied boolean.
//!
//! What it pins:
//! * the vendored copy still matches its `.sha256` pin (the repo-level drift check compares it
//!   with the canonical copy; this is the consumer side of the same pin);
//! * every provider row that names an inference-core provider agrees with that provider's real
//!   `capabilities()`: a connection is `supported` exactly when the provider reports
//!   `supports_zdr`;
//! * a provider the contract calls anything but `supported` refuses a ZDR request with
//!   `ZdrUnavailable` before it touches the network, so the contract's `not_supported` is true
//!   of the code and not only of the table;
//! * the Rust half of "loaders reject unknown fields": the typed `Contract` below denies unknown
//!   fields at every level, so a field added to the golden that this plane was never taught about
//!   fails here instead of being ignored, and a missing field fails too.
//!
//! A row whose inference provider this test cannot instantiate FAILS the test: a contract row
//! nobody checks against the code is exactly how the table and the providers drift apart. Add
//! the provider to `real_provider` in the change that adds the row.

use inference_core::provider::codex_subscription::CodexSubscriptionProvider;
use inference_core::provider::{InferRequest, ProviderError, ProviderRouter};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

const CONTRACT: &str = include_str!("fixtures/contracts/connection-zdr-capabilities.json");
const PIN: &str = include_str!("fixtures/contracts/connection-zdr-capabilities.json.sha256");

fn contract() -> Value {
    serde_json::from_str(CONTRACT).expect("the vendored capability contract is JSON")
}

/// The real provider behind an inference provider id. No network is configured: the broker URL
/// is unreachable on purpose, so a test that reached it would fail with an unavailable error
/// instead of the typed ZDR refusal it expects.
fn real_provider(provider_id: &str) -> CodexSubscriptionProvider {
    match provider_id {
        "openai-codex-subscription" => CodexSubscriptionProvider::new(
            "http://127.0.0.1:1",
            "contract-test-key",
            vec!["gpt-5".to_owned()],
        )
        .expect("the real subscription provider builds from explicit configuration"),
        other => panic!(
            "the capability contract names inference provider `{other}` but this test cannot \
             instantiate it: add it to `real_provider` so the row is checked against the code"
        ),
    }
}

/// The provider rows that have an inference-core counterpart.
fn rows_with_a_real_provider() -> Vec<Value> {
    let rows: Vec<Value> = contract()["providers"]
        .as_array()
        .expect("providers is a list")
        .iter()
        .filter(|row| !row["inference_provider_id"].is_null())
        .cloned()
        .collect();
    assert!(
        !rows.is_empty(),
        "no provider row names an inference-core provider, so nothing is being cross-checked"
    );
    rows
}

#[test]
fn the_vendored_copy_matches_its_pin() {
    let normalised = CONTRACT.replace("\r\n", "\n");
    let digest = format!("{:x}", Sha256::digest(normalised.as_bytes()));
    let pinned = PIN
        .split_whitespace()
        .next()
        .expect("the pin file starts with a digest");
    assert_eq!(
        digest, pinned,
        "the vendored connection-zdr-capabilities.json differs from the digest pinned beside it; \
         re-vendor it from apps/Model Plane/api/contracts and refresh the pin"
    );
}

#[test]
fn the_contract_is_the_format_this_test_understands() {
    let doc = contract();
    assert_eq!(doc["contract"], "connection.zdr-capabilities");
    assert_eq!(doc["version"], 1, "format changed: review this test");
    assert!(
        doc["catalog_version"].as_u64().is_some_and(|v| v >= 1),
        "catalog_version is a positive integer"
    );
}

#[test]
fn every_provider_row_agrees_with_the_real_provider_capabilities() {
    for row in rows_with_a_real_provider() {
        let id = row["inference_provider_id"]
            .as_str()
            .expect("inference_provider_id is a string");
        let basis_ref = row["basis_ref"].as_str().expect("basis_ref is a string");
        let capabilities = real_provider(id).capabilities();

        assert_eq!(
            capabilities.provider_id, id,
            "{basis_ref}: the contract names `{id}` but the real provider calls itself `{}`",
            capabilities.provider_id
        );
        assert_eq!(
            capabilities.supports_zdr,
            row["inference_supports_zdr"]
                .as_bool()
                .expect("inference_supports_zdr is a boolean"),
            "{basis_ref}: the contract's inference_supports_zdr differs from the real provider"
        );
        let supported = row["capability"] == "supported";
        assert_eq!(
            capabilities.supports_zdr, supported,
            "{basis_ref}: a connection is `supported` exactly when its provider reports \
             supports_zdr (capability is {})",
            row["capability"]
        );
    }
}

#[tokio::test]
async fn a_provider_not_called_supported_refuses_zdr_before_any_network_call() {
    for row in rows_with_a_real_provider() {
        if row["capability"] == "supported" {
            continue;
        }
        let id = row["inference_provider_id"]
            .as_str()
            .expect("inference_provider_id is a string");
        let provider = real_provider(id);
        let request = InferRequest {
            request_id: "contract-test".to_owned(),
            model: "gpt-5".to_owned(),
            zdr: true,
            subscription_connection_id: "conn-1".to_owned(),
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            ..InferRequest::default()
        };
        match provider.infer(&request).await {
            Err(ProviderError::ZdrUnavailable(_)) => {}
            other => panic!(
                "{}: the contract says {} but a ZDR request to the real provider did not end in \
                 ZdrUnavailable: {other:?}",
                row["basis_ref"], row["capability"]
            ),
        }
    }
}

/// The streaming path is the one chat takes, and it is a separate method from `infer`: a provider
/// that refuses ZDR on one and serves it on the other would pass a test of only `infer`.
#[tokio::test]
async fn a_provider_not_called_supported_refuses_a_zdr_stream_before_any_network_call() {
    for row in rows_with_a_real_provider() {
        if row["capability"] == "supported" {
            continue;
        }
        let id = row["inference_provider_id"]
            .as_str()
            .expect("inference_provider_id is a string");
        let provider = real_provider(id);
        let request = InferRequest {
            request_id: "contract-test-stream".to_owned(),
            model: "gpt-5".to_owned(),
            zdr: true,
            subscription_connection_id: "conn-1".to_owned(),
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            ..InferRequest::default()
        };
        // The receiver is not `Debug`, so match instead of formatting the whole result.
        match provider.infer_stream(&request).await {
            Err(ProviderError::ZdrUnavailable(_)) => {}
            Err(other) => panic!(
                "{}: the contract says {} but a ZDR stream request to the real provider did not end \
                 in ZdrUnavailable: {other:?}",
                row["basis_ref"], row["capability"]
            ),
            Ok(_) => panic!(
                "{}: the contract says {} but the real provider opened a stream for a ZDR request",
                row["basis_ref"], row["capability"]
            ),
        }
    }
}

#[test]
fn the_fallback_never_rounds_an_unassessed_connection_up_to_supported() {
    let doc = contract();
    assert_eq!(doc["fallback"]["capability"], "cannot_verify");
    assert_eq!(doc["fallback"]["waiver_route"], false);
    let stays_on = doc["run_rule"]["zdr_stays_on"]
        .as_array()
        .expect("run_rule.zdr_stays_on is a list");
    assert_eq!(stays_on, &[Value::from("supported")]);
}

// ---- the strict, typed loader ---------------------------------------------------------------

/// Every struct below denies unknown fields. The fields that no assertion reads exist only to make
/// the loader strict, which is why dead code is allowed on them.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct Contract {
    #[serde(rename = "contract")]
    kind: String,
    version: u32,
    catalog_version: u32,
    description: String,
    states: Vec<String>,
    bases: Vec<String>,
    run_rule: RunRule,
    locales: Vec<String>,
    wording_variants: Vec<String>,
    wording_keys: Vec<WordingKey>,
    digest: DigestSpec,
    providers: Vec<ProviderRow>,
    fallback: Fallback,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunRule {
    zdr_stays_on: Vec<String>,
    acknowledgement_required: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct WordingKey {
    key: String,
    state: String,
    variants: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct DigestSpec {
    algorithm: String,
    encoding: String,
    fields: Vec<String>,
    canonical_form: String,
    vectors: Vec<DigestVector>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct DigestVector {
    id: String,
    note: String,
    input: Value,
    canonical: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct ProviderRow {
    basis_ref: String,
    provider_key: String,
    auth_mode: String,
    capability: String,
    basis: String,
    waiver_route: bool,
    wording_key: String,
    inference_provider_id: Option<String>,
    inference_supports_zdr: Option<bool>,
    evidence: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fallback {
    basis_ref: String,
    capability: String,
    basis: String,
    waiver_route: bool,
    wording_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    input: CaseInput,
    expect: CaseExpect,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseInput {
    provider_key: String,
    auth_mode: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseExpect {
    basis_ref: String,
    capability: String,
    basis: String,
    waiver_route: bool,
    wording_key: String,
}

fn typed() -> Contract {
    serde_json::from_str(CONTRACT).expect("the vendored capability contract loads strictly")
}

/// What the strict loader says about the vendored contract after `mutate` changed it.
fn loader_error_after(mutate: impl FnOnce(&mut Value)) -> String {
    let mut doc = contract();
    mutate(&mut doc);
    serde_json::from_value::<Contract>(doc)
        .expect_err("the strict loader must reject the mutated contract")
        .to_string()
}

#[test]
fn the_strict_loader_rejects_an_unknown_field_at_every_level() {
    type Mutation = fn(&mut Value);
    let mutations: Vec<(&str, Mutation)> = vec![
        ("top level", |d| d["surprise"] = Value::Bool(true)),
        ("run_rule", |d| {
            d["run_rule"]["surprise"] = Value::Bool(true);
        }),
        ("wording_keys[]", |d| {
            d["wording_keys"][0]["surprise"] = Value::Bool(true);
        }),
        ("digest", |d| d["digest"]["surprise"] = Value::Bool(true)),
        ("digest.vectors[]", |d| {
            d["digest"]["vectors"][0]["surprise"] = Value::Bool(true);
        }),
        ("providers[]", |d| {
            d["providers"][0]["surprise"] = Value::Bool(true);
        }),
        ("fallback", |d| {
            d["fallback"]["surprise"] = Value::Bool(true);
        }),
        ("cases[]", |d| d["cases"][0]["surprise"] = Value::Bool(true)),
        ("cases[].input", |d| {
            d["cases"][0]["input"]["surprise"] = Value::Bool(true);
        }),
        ("cases[].expect", |d| {
            d["cases"][0]["expect"]["surprise"] = Value::Bool(true);
        }),
    ];
    for (level, mutate) in mutations {
        let message = loader_error_after(mutate);
        assert!(
            message.contains("unknown field `surprise`"),
            "an unknown field at {level} must be rejected as such, got: {message}"
        );
    }
}

#[test]
fn the_strict_loader_rejects_a_missing_field() {
    let message = loader_error_after(|d| {
        d["providers"][0]
            .as_object_mut()
            .expect("a provider row is an object")
            .remove("capability");
    });
    assert!(
        message.contains("missing field `capability`"),
        "a provider row without a capability must be rejected, got: {message}"
    );
    let message = loader_error_after(|d| {
        d["fallback"]
            .as_object_mut()
            .expect("the fallback is an object")
            .remove("waiver_route");
    });
    assert!(
        message.contains("missing field `waiver_route`"),
        "a fallback without waiver_route must be rejected, got: {message}"
    );
}

/// The state a wording key speaks about, when the contract defines the key.
fn wording_state(c: &Contract, key: &str) -> Option<String> {
    c.wording_keys
        .iter()
        .find(|w| w.key == key)
        .map(|w| w.state.clone())
}

#[test]
fn every_provider_row_and_the_fallback_use_only_what_the_contract_defines() {
    let c = typed();
    assert_eq!(c.kind, "connection.zdr-capabilities");
    assert_eq!(c.version, 1, "format changed: review this test");
    assert!(c.catalog_version >= 1, "catalog_version is positive");
    assert_eq!(c.states, ["supported", "not_supported", "cannot_verify"]);
    assert_eq!(c.run_rule.zdr_stays_on, ["supported"]);
    for state in &c.run_rule.acknowledgement_required {
        assert!(
            c.states.contains(state),
            "run_rule names unknown state {state}"
        );
    }
    let covers_every_state_once: usize =
        c.run_rule.zdr_stays_on.len() + c.run_rule.acknowledgement_required.len();
    assert_eq!(
        covers_every_state_once,
        c.states.len(),
        "run_rule covers each state exactly once"
    );

    for p in &c.providers {
        assert!(
            c.states.contains(&p.capability),
            "{}: unknown state {}",
            p.basis_ref,
            p.capability
        );
        assert!(
            c.bases.contains(&p.basis),
            "{}: unknown basis {}",
            p.basis_ref,
            p.basis
        );
        assert_eq!(
            wording_state(&c, &p.wording_key).as_deref(),
            Some(p.capability.as_str()),
            "{}: its wording key must exist and speak about the same state",
            p.basis_ref
        );
    }
    assert_eq!(
        wording_state(&c, &c.fallback.wording_key).as_deref(),
        Some("cannot_verify"),
        "the fallback's wording key is a cannot_verify text"
    );
    assert!(
        c.bases.contains(&c.fallback.basis),
        "the fallback names a known basis"
    );
    assert!(
        c.states.contains(&c.fallback.capability),
        "the fallback names a known state"
    );
    assert!(
        !c.fallback.basis_ref.is_empty() && !c.fallback.waiver_route,
        "the fallback has no waiver route"
    );
}

#[test]
fn every_case_expects_what_the_contract_defines() {
    let c = typed();
    for case in &c.cases {
        let known_ref = c
            .providers
            .iter()
            .any(|p| p.basis_ref == case.expect.basis_ref)
            || c.fallback.basis_ref == case.expect.basis_ref;
        assert!(
            known_ref,
            "case {}: expects an unknown basis_ref {}",
            case.id, case.expect.basis_ref
        );
        assert!(
            c.states.contains(&case.expect.capability),
            "case {}: unknown state",
            case.id
        );
        assert!(
            c.bases.contains(&case.expect.basis),
            "case {}: unknown basis",
            case.id
        );
        assert!(
            wording_state(&c, &case.expect.wording_key).is_some(),
            "case {}: unknown wording key {}",
            case.id,
            case.expect.wording_key
        );
        // An unassessed pair never gets a waiver route: the fallback is the only way to be unknown.
        if case.expect.basis_ref == c.fallback.basis_ref {
            assert!(
                !case.expect.waiver_route,
                "case {}: the fallback has no waiver route",
                case.id
            );
            assert!(
                !c.providers
                    .iter()
                    .any(|p| p.provider_key == case.input.provider_key
                        && p.auth_mode == case.input.auth_mode),
                "case {}: expects the fallback for a pair the contract does assess",
                case.id
            );
        }
    }
}

/// Canonical JSON as the contract defines it: keys in ascending order, no whitespace outside
/// strings. Only integration-corev2 computes a real digest; this is the Rust reading of the
/// vectors, so the golden cannot quietly depend on one language's serialiser.
fn canonical(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let members: Vec<String> = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).expect("a key serialises"),
                        canonical(&map[key])
                    )
                })
                .collect();
            format!("{{{}}}", members.join(","))
        }
        Value::Array(items) => {
            let members: Vec<String> = items.iter().map(canonical).collect();
            format!("[{}]", members.join(","))
        }
        scalar => serde_json::to_string(scalar).expect("a scalar serialises"),
    }
}

#[test]
fn the_digest_vectors_hash_the_same_way_in_rust() {
    let c = typed();
    assert_eq!(c.digest.algorithm, "sha256");
    let mut sorted_fields = c.digest.fields.clone();
    sorted_fields.sort();
    assert_eq!(
        c.digest.fields, sorted_fields,
        "digest.fields are in ascending order"
    );
    assert!(
        c.digest.vectors.len() >= 3,
        "a vector for the current digest, a version bump and a capability change"
    );

    let mut seen = std::collections::HashSet::new();
    for vector in &c.digest.vectors {
        let mut input_fields: Vec<&str> = vector
            .input
            .as_object()
            .unwrap_or_else(|| panic!("{}: input is an object", vector.id))
            .keys()
            .map(String::as_str)
            .collect();
        input_fields.sort_unstable();
        assert_eq!(
            input_fields, c.digest.fields,
            "{}: the input holds exactly the digest fields",
            vector.id
        );
        let rendered = canonical(&vector.input);
        assert_eq!(rendered, vector.canonical, "{}: canonical form", vector.id);
        let digest = format!("{:x}", Sha256::digest(rendered.as_bytes()));
        assert_eq!(
            digest, vector.sha256,
            "{}: sha256 of the canonical form",
            vector.id
        );
        assert!(
            seen.insert(digest),
            "{}: two vectors share a digest, so one shows nothing",
            vector.id
        );
    }
}
