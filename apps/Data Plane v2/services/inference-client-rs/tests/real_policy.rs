//! The deployment against the real policy.
//!
//! Three things have to agree for a worker to get a bearer: the principal row in
//! Auth Core's registry, the compose file that wires the worker, and the client's
//! own configuration vocabulary. When they drifted, embedding died silently (the
//! registry moved to `persistent` while the clients still demanded `zdr`) and no
//! test of the client alone could see it, because each test chose the values it
//! was happy with. This file reads the real ones.
//!
//! * The registry rows are the ones Auth Core's mint was captured with
//!   (`tests/fixtures/auth-core/inference-service-tokens.golden.json`, `registry`,
//!   vendored with a sha256 pin and compared by Auth Core's own spec with
//!   `config/plane-service-principals.json`). When the Control Plane file is
//!   reachable from this checkout the rows are compared with it directly; under
//!   `REQUIRE_CONTRACT` or `CI` it must be reachable, or this fails.
//! * The compose file, `.env.example` and the Makefile are read from this tree.
//!   They are not vendored, and a missing one fails: they sit next to the crate.
//!
//! Nothing here skips except the live comparison with the Control Plane file when
//! neither `REQUIRE_CONTRACT` nor `CI` is set.

mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;

use common::{require_contract, Golden};
use inference_client_rs::{Reason, RetentionPosture};
use serde_json::Value;

const SERVICES: [&str; 2] = ["embedding-engine", "graph-index"];

fn data_plane_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn read(relative: &str) -> String {
    let path = data_plane_root().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is missing ({error})", path.display()))
}

fn registry() -> BTreeMap<String, Value> {
    Golden::load().value()["registry"]
        .as_object()
        .expect("registry")
        .iter()
        .map(|(id, row)| (id.clone(), row.clone()))
        .collect()
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("expected an array, got {value}"))
        .iter()
        .map(|item| item.as_str().expect("string").to_owned())
        .collect()
}

// ───────────────────────── the compose file ─────────────────────────

/// The lines of one service's block of `docker-compose.yml`.
fn service_block<'a>(compose: &'a str, service: &str) -> Vec<&'a str> {
    let header = format!("  {service}:");
    let mut inside = false;
    let mut block = Vec::new();
    for line in compose.lines() {
        if !inside {
            inside = line.trim_end() == header;
            continue;
        }
        let is_service_key = line.starts_with("  ")
            && !line.starts_with("   ")
            && !line.trim_start().starts_with('#')
            && line.trim_end().ends_with(':');
        let is_top_level = !line.is_empty() && !line.starts_with(' ') && !line.starts_with('#');
        if is_service_key || is_top_level {
            break;
        }
        block.push(line);
    }
    assert!(
        !block.is_empty(),
        "service {service} not found in docker-compose.yml"
    );
    block
}

/// `NAME: ${HOST_VAR:-default}` inside a service block, as (host variable, default).
fn compose_env(block: &[&str], name: &str) -> (String, Option<String>) {
    let prefix = format!("{name}:");
    let line = block
        .iter()
        .map(|line| line.trim())
        .find(|line| line.starts_with(&prefix))
        .unwrap_or_else(|| panic!("{name} is not set in the service block"));
    let value = line[prefix.len()..].trim();
    let inner = value
        .strip_prefix("${")
        .and_then(|rest| rest.strip_suffix('}'))
        .unwrap_or_else(|| panic!("{name} is not an interpolation: {value}"));
    match inner.split_once(":-") {
        Some((host, default)) => (host.to_owned(), Some(default.to_owned())),
        None => (inner.to_owned(), None),
    }
}

/// Posture variables per service: (container variable, host variable) as the
/// client reads them and as the operator sets them.
fn posture_vars() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        (
            "embedding-engine",
            "MODEL_PLANE_EMBEDDING_INFERENCE_RETENTION_POSTURE",
        ),
        ("graph-index", "MODEL_PLANE_INFERENCE_RETENTION_POSTURE"),
    ])
}

const CONTAINER_POSTURE_VAR: &str = "MODEL_PLANE_INFERENCE_RETENTION_POSTURE";

/// How a client posture sits against a registry row.
#[derive(Debug, PartialEq, Eq)]
enum Fit {
    /// The same posture on both sides.
    Matches,
    /// A `tenant` client accepts either value of the claim, so it works against
    /// every row. That is why the clients ship first.
    ToleratesAny,
    /// The registry row says `tenant` and the client still asserts `persistent`.
    /// Non-ZDR organizations are unaffected (their token carries `zdr:false`);
    /// an organization with ZDR on gets `zdr:true`, which the client refuses, so
    /// its work waits and nothing is sent. This is the window between the
    /// registry change and the enablement step that sets the posture variable to
    /// `tenant`. It is tolerated, because the variable is flipped as a separate
    /// step after the Control Plane image has the tenant code, and it is said out
    /// loud rather than passed in silence.
    RegistryAhead,
    /// The client would refuse tokens that organizations with no ZDR get, or the
    /// other way round, on every mint: embedding dies silently.
    Broken,
}

fn fit(client: RetentionPosture, registry: RetentionPosture) -> Fit {
    use RetentionPosture::{Persistent, Tenant};
    if client == registry {
        Fit::Matches
    } else if client == Tenant {
        Fit::ToleratesAny
    } else if client == Persistent && registry == Tenant {
        Fit::RegistryAhead
    } else {
        Fit::Broken
    }
}

#[test]
fn the_fit_rule_is_the_staged_rollout() {
    use RetentionPosture::{Persistent, Tenant, ZeroRetention};
    // Clients first: the new client works against every row...
    assert_eq!(fit(Tenant, Persistent), Fit::ToleratesAny);
    assert_eq!(fit(Tenant, ZeroRetention), Fit::ToleratesAny);
    assert_eq!(fit(Tenant, Tenant), Fit::Matches);
    assert_eq!(fit(Persistent, Persistent), Fit::Matches);
    assert_eq!(fit(ZeroRetention, ZeroRetention), Fit::Matches);
    // ...a client still on `persistent` is a stall for ZDR organizations only...
    assert_eq!(fit(Persistent, Tenant), Fit::RegistryAhead);
    // ...and these are the ones that kill every mint.
    assert_eq!(fit(ZeroRetention, Tenant), Fit::Broken);
    assert_eq!(fit(ZeroRetention, Persistent), Fit::Broken);
    assert_eq!(fit(Persistent, ZeroRetention), Fit::Broken);
}

#[test]
fn the_registry_rows_say_what_the_client_assumes() {
    let registry = registry();
    for service in SERVICES {
        let row = registry
            .get(service)
            .unwrap_or_else(|| panic!("the registry slice has no {service}"));
        assert_eq!(strings(&row["audiences"]), ["inference-core"], "{service}");
        assert_eq!(strings(&row["scopes"]), ["inference:invoke"], "{service}");
        assert_eq!(
            strings(&row["scopesByAudience"]["inference-core"]),
            ["inference:invoke"],
            "{service}"
        );
        // The production principal model: bound to the organization directory (or
        // to fixed organizations), never "any organization".
        assert!(
            row.get("allowAnyOrg")
                .is_none_or(|value| value == &Value::Bool(false)),
            "{service}: allowAnyOrg is refused in production file mode"
        );
        assert!(
            row["tenancy"] == "directory"
                || row["orgIds"].as_array().is_some_and(|ids| !ids.is_empty()),
            "{service} has neither directory tenancy nor fixed organizations"
        );
        let retention = row["retentionByAudience"]["inference-core"]
            .as_str()
            .unwrap_or_else(|| panic!("{service} has no retention for inference-core"));
        RetentionPosture::parse(retention).unwrap_or_else(|error| {
            panic!("{service}: the registry vocabulary and the client's differ: {error}")
        });
    }
}

#[test]
fn compose_wires_each_service_to_its_own_principal_and_the_registered_audience() {
    let compose = read("docker-compose.yml");
    let registry = registry();
    let api_key_vars = BTreeMap::from([
        (
            "embedding-engine",
            "MODEL_PLANE_EMBEDDING_INFERENCE_SERVICE_API_KEY",
        ),
        ("graph-index", "MODEL_PLANE_INFERENCE_SERVICE_API_KEY"),
    ]);
    for service in SERVICES {
        let block = service_block(&compose, service);

        // The service id the client sends is the registered principal.
        let (_, id_default) = compose_env(&block, "MODEL_PLANE_INFERENCE_SERVICE_ID");
        assert_eq!(id_default.as_deref(), Some(service), "{service}");

        // The credential comes from the variable the registry names for it, so a
        // rename on one side cannot go unnoticed.
        let (key_host, key_default) = compose_env(&block, "MODEL_PLANE_INFERENCE_SERVICE_API_KEY");
        assert_eq!(
            Some(key_host.as_str()),
            registry[service]["consumerEnv"].as_str(),
            "{service}: the compose credential variable and the registry's consumerEnv differ"
        );
        assert_eq!(key_host.as_str(), api_key_vars[service], "{service}");
        assert_eq!(
            key_default.as_deref(),
            Some(""),
            "{service}: a credential must never have a default"
        );

        // The token endpoint is the one for the audience the row is registered for.
        let (_, url_default) = compose_env(&block, "MODEL_PLANE_INFERENCE_TOKEN_URL");
        assert!(
            url_default
                .as_deref()
                .is_some_and(|url| url.ends_with("/api/inference-core/internal-token")),
            "{service}: {url_default:?}"
        );
    }
}

#[test]
fn the_two_posture_variables_are_distinct_and_each_agrees_with_its_registry_row() {
    let compose = read("docker-compose.yml");
    let registry = registry();
    let vars = posture_vars();
    assert_ne!(
        vars["embedding-engine"], vars["graph-index"],
        "the two services are set by different host variables; setting one must not move the other"
    );
    for service in SERVICES {
        let block = service_block(&compose, service);
        let (host, default) = compose_env(&block, CONTAINER_POSTURE_VAR);
        assert_eq!(
            host, vars[service],
            "{service}: the operator-facing variable"
        );
        let default = default.unwrap_or_else(|| panic!("{service}: the posture has no default"));
        let client = RetentionPosture::parse(&default)
            .unwrap_or_else(|error| panic!("{service}: compose default `{default}`: {error}"));
        let row = registry[service]["retentionByAudience"]["inference-core"]
            .as_str()
            .expect("retention");
        let row = RetentionPosture::parse(row).expect("registry posture");
        let fit = fit(client, row);
        assert_ne!(
            fit,
            Fit::Broken,
            "{service}: compose defaults the client to `{}` but the registry row says `{}`: \
             every mint would fail. Change them in one release (clients first, then the registry)",
            client.as_str(),
            row.as_str()
        );
        if fit == Fit::RegistryAhead {
            eprintln!(
                "note: {service}: the registry row says `tenant` and the compose default is still `{}`: \
                 set {} to `tenant` at the enablement step, or organizations with ZDR on wait",
                client.as_str(),
                vars[service]
            );
        }
    }
}

#[test]
fn env_example_documents_both_posture_variables_with_the_compose_default() {
    let compose = read("docker-compose.yml");
    let example = read(".env.example");
    for (service, variable) in posture_vars() {
        let block = service_block(&compose, service);
        let (_, default) = compose_env(&block, CONTAINER_POSTURE_VAR);
        let line = example
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with(&format!("{variable}=")))
            .unwrap_or_else(|| panic!(".env.example does not document {variable}"));
        assert_eq!(
            line.split_once('=').map(|(_, value)| value),
            default.as_deref(),
            "{variable}: .env.example and the compose default differ"
        );
    }
    // The vocabulary is described where an operator looks for it.
    assert!(
        example.contains("tenant"),
        ".env.example must describe the `tenant` posture next to the variables"
    );
}

#[test]
fn the_makefile_checks_the_posture_values_before_a_connected_start() {
    let makefile = read("Makefile");
    for variable in posture_vars().into_values() {
        assert!(
            makefile.contains(variable),
            "make cross-plane-env must validate {variable}"
        );
    }
    assert!(
        makefile.contains("persistent|zdr|tenant"),
        "make cross-plane-env must name the vocabulary it accepts"
    );
}

#[test]
fn make_cross_plane_env_requires_the_keys_the_usage_ledger_adds() {
    // Event keys and the cost-core principal credential are provisioned by hand
    // through .env and the Makefile list; the launcher does not provision them.
    // The Makefile hunk for them is made here because the same chain edits the same
    // line (CR-T10 wires the compose side).
    let makefile = read("Makefile");
    for name in [
        "DATA_ORCHESTRATOR_COST_SERVICE_API_KEY",
        "GRAPH_EVENT_SIGNING_PRIVATE_KEY_PATH",
        "GRAPH_EVENT_VERIFYING_PUBLIC_KEY_PATH",
    ] {
        assert!(
            makefile.contains(name),
            "{name} is not in the cross-plane-env list"
        );
    }
}

#[test]
fn the_registry_slice_is_the_one_in_the_control_plane_file() {
    let path = data_plane_root().join("../Control Plane/config/plane-service-principals.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if require_contract() => panic!(
            "REQUIRE_CONTRACT/CI is set but {} is unreadable ({error})",
            path.display()
        ),
        Err(_) => {
            eprintln!("skipped: the Control Plane tree is not part of this checkout");
            return;
        }
    };
    let live: Value = serde_json::from_str(&text).expect("registry is JSON");
    let slice = registry();
    for service in SERVICES {
        assert_eq!(
            slice[service], live[service],
            "{service}: the registry changed in the Control Plane. Regenerate the vendored golden \
             (UPDATE_CONTRACT_GOLDEN=1 npx jest inference-service-token.contract in auth-core) and \
             re-read the compose posture defaults"
        );
    }
}

#[test]
fn the_services_use_this_crate_and_no_copy_of_the_client_remains() {
    // The condition of gate G-T5-DONE: the shared crate is in use and the two
    // per-service copies are gone. (The retrieval engine's own copy is a separate,
    // later convergence.)
    for (service, source) in [
        ("embedding-engine-rs", "src/provider/inference_auth.rs"),
        ("graph-index-rs", "src/inference_auth.rs"),
    ] {
        let manifest = read(&format!("services/{service}/Cargo.toml"));
        assert!(
            manifest.contains("inference-client-rs"),
            "{service} does not depend on inference-client-rs"
        );
        assert!(
            !data_plane_root()
                .join(format!("services/{service}/{source}"))
                .exists(),
            "{service} still carries its own copy of the token client ({source})"
        );
    }
}

#[test]
fn every_reason_a_service_mints_with_is_in_the_vocabulary() {
    // The features that call inference-core from the Data Plane, one reason each.
    let golden = Golden::load();
    let mut reasons: Vec<&str> = Reason::ALL.iter().map(|reason| reason.as_str()).collect();
    reasons.sort_unstable();
    let mut captured = golden.reasons();
    captured.sort();
    assert_eq!(reasons, captured);
}
