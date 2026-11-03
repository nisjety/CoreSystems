//! The Agent API v1 contract (EP-12-S3). The public OpenAPI document and its golden
//! agree, and the error vocabulary they share with the ZDR and model entitlement
//! catalogues is the one those catalogues hold.
//!
//! `api/contracts/agent-api-v1.json` is the golden: the six routes (method, public path,
//! gateway path, scope, operation id, parameters and body fields), the response field
//! sets, the limits, the refused fields, and the closed error vocabulary with each code's
//! status and details keys. `api/agent-api.v1.yaml` is the document the public SDKs are
//! generated from. This test reads both and fails when they disagree, and when the
//! golden changes without its `.sha256` pin being refreshed.
//!
//! The spec is read with a small reader for the block style it is written in, not a YAML
//! parser: the gateway has no YAML dependency and this check does not add one. The reader
//! supports exactly what the spec uses (nested keys, `- ` items, one-line values), and a
//! lookup that finds nothing fails the test rather than passing it.

use mp_contracts::zdr_refusal;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const GOLDEN: &str = include_str!("../../../../api/contracts/agent-api-v1.json");
const PIN: &str = include_str!("../../../../api/contracts/agent-api-v1.json.sha256");
const SPEC: &str = include_str!("../../../../api/agent-api.v1.yaml");
const ENTITLEMENT_REFUSALS: &str =
    include_str!("../../../../api/contracts/model-entitlement-refusals.json");

/// One line of the spec: its indentation and its text. Blank lines and comments are
/// dropped, so every line the reader sees says something.
#[derive(Clone, Copy)]
struct Line<'a> {
    indent: usize,
    text: &'a str,
}

fn lines(document: &str) -> Vec<Line<'_>> {
    document
        .lines()
        .filter_map(|raw| {
            let text = raw.trim_end();
            let body = text.trim_start();
            if body.is_empty() || body.starts_with('#') {
                return None;
            }
            Some(Line {
                indent: text.len() - body.len(),
                text: body,
            })
        })
        .collect()
}

/// The lines under the key `name` at the block's own level, up to the first line that
/// is not indented beneath that key.
fn child<'a>(block: &[Line<'a>], name: &str) -> Option<Vec<Line<'a>>> {
    let level = block.iter().map(|line| line.indent).min()?;
    let index = block
        .iter()
        .position(|line| line.indent == level && line.text.strip_suffix(':') == Some(name))?;
    let key_indent = block[index].indent;
    let below = &block[index + 1..];
    let length = below
        .iter()
        .position(|line| line.indent <= key_indent)
        .unwrap_or(below.len());
    Some(below[..length].to_vec())
}

/// The block under a path of keys, each key inside the one before it.
fn at<'a>(document: &[Line<'a>], path: &[&str]) -> Vec<Line<'a>> {
    path.iter().fold(document.to_vec(), |block, name| {
        child(&block, name).unwrap_or_else(|| panic!("the spec has no {name} under {path:?}"))
    })
}

/// The value written on the same line as the key `name`, at the block's own level.
fn scalar<'a>(block: &[Line<'a>], name: &str) -> Option<&'a str> {
    let level = block.iter().map(|line| line.indent).min()?;
    block
        .iter()
        .filter(|line| line.indent == level)
        .find_map(|line| line.text.strip_prefix(name)?.strip_prefix(':').map(str::trim))
        .filter(|value| !value.is_empty())
}

/// The keys written at the block's own level, in document order.
fn keys<'a>(block: &[Line<'a>]) -> Vec<&'a str> {
    let Some(level) = block.iter().map(|line| line.indent).min() else {
        return Vec::new();
    };
    block
        .iter()
        .filter(|line| line.indent == level)
        .filter_map(|line| line.text.strip_suffix(':'))
        .collect()
}

/// The `- ` items written under the key `name`, without their marker.
fn items<'a>(block: &[Line<'a>], name: &str) -> Option<Vec<&'a str>> {
    let under = child(block, name)?;
    Some(
        under
            .iter()
            .filter_map(|line| line.text.strip_prefix("- "))
            .collect(),
    )
}

/// The `- name:` parameters of an operation as (location, name), in document order.
fn parameters(operation: &[Line<'_>]) -> Vec<(String, String)> {
    let Some(block) = child(operation, "parameters") else {
        return Vec::new();
    };
    let mut found: Vec<(String, String)> = Vec::new();
    for line in &block {
        if let Some(name) = line.text.strip_prefix("- name: ") {
            found.push((String::new(), name.to_owned()));
        } else if let Some(location) = line.text.strip_prefix("in: ") {
            if let Some(last) = found.last_mut() {
                last.0 = location.to_owned();
            }
        }
    }
    found
}

/// The `{name}` parameters of a path template.
fn template_parameters(path: &str) -> BTreeSet<String> {
    path.split('{')
        .skip(1)
        .filter_map(|rest| rest.split('}').next())
        .map(str::to_owned)
        .collect()
}

fn set_of<'a>(values: impl IntoIterator<Item = &'a str>) -> BTreeSet<String> {
    values.into_iter().map(str::to_owned).collect()
}

fn golden() -> Value {
    serde_json::from_str(GOLDEN).expect("agent-api-v1.json is JSON")
}

/// The text items of a JSON list.
fn texts(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("a list of text")
        .iter()
        .map(|item| item.as_str().expect("a text item").to_owned())
        .collect()
}

fn golden_set(value: &Value) -> BTreeSet<String> {
    texts(value).into_iter().collect()
}

#[test]
fn golden_and_spec_name_the_same_contract_version_and_prefix() {
    let golden = golden();
    assert_eq!(golden["contract"].as_str(), Some("model-plane.agent-api"));
    assert_eq!(golden["version"].as_u64(), Some(1));

    let document = lines(SPEC);
    let info = at(&document, &["info"]);
    assert_eq!(scalar(&info, "version"), golden["spec"]["version"].as_str());

    let server = document
        .iter()
        .find(|line| line.text.starts_with("- url:"))
        .expect("the spec names a server");
    assert_eq!(
        Some(server.text.trim_start_matches("- url:").trim()),
        golden["spec"]["public_prefix"].as_str()
    );
}

#[test]
fn spec_paths_and_methods_are_the_golden_routes() {
    let golden = golden();
    let routes = golden["routes"].as_array().expect("routes is a list");
    let document = lines(SPEC);
    let paths = at(&document, &["paths"]);

    let mut declared: Vec<(String, String)> = Vec::new();
    for path in keys(&paths) {
        for method in keys(&at(&paths, &[path])) {
            if ["get", "post", "put", "patch", "delete"].contains(&method) {
                declared.push((method.to_ascii_uppercase(), path.to_owned()));
            }
        }
    }
    let mut expected: Vec<(String, String)> = routes
        .iter()
        .map(|route| {
            (
                route["method"].as_str().expect("a method").to_owned(),
                route["path"].as_str().expect("a path").to_owned(),
            )
        })
        .collect();
    expected.sort();
    declared.sort();
    assert_eq!(
        declared, expected,
        "the spec's paths and methods are the golden's routes"
    );

    for route in routes {
        let method = route["method"]
            .as_str()
            .expect("a method")
            .to_ascii_lowercase();
        let path = route["path"].as_str().expect("a path");
        let label = format!("{} {path}", method.to_ascii_uppercase());
        let operation = at(&paths, &[path, method.as_str()]);

        assert_eq!(
            scalar(&operation, "operationId"),
            route["operation_id"].as_str(),
            "{label}: operationId"
        );
        assert_eq!(
            scalar(&operation, "x-gateway-path"),
            route["gateway_path"].as_str(),
            "{label}: x-gateway-path"
        );
        assert_eq!(
            scalar(&operation, "x-scope"),
            route["scope"].as_str(),
            "{label}: x-scope"
        );

        let mut spec_parameters = parameters(&operation);
        let mut golden_parameters: Vec<(String, String)> = template_parameters(path)
            .into_iter()
            .map(|name| ("path".to_owned(), name))
            .collect();
        golden_parameters.extend(
            texts(&route["query_fields"])
                .into_iter()
                .map(|name| ("query".to_owned(), name)),
        );
        golden_parameters.extend(
            texts(&route["headers"])
                .into_iter()
                .map(|name| ("header".to_owned(), name)),
        );
        spec_parameters.sort();
        golden_parameters.sort();
        assert_eq!(spec_parameters, golden_parameters, "{label}: parameters");

        assert_eq!(
            child(&operation, "requestBody").is_some(),
            !texts(&route["body_fields"]).is_empty(),
            "{label}: a request body exists exactly when the route takes body fields"
        );
    }
}

#[test]
fn the_error_vocabulary_is_one_closed_list_shared_with_the_catalogues() {
    let golden = golden();
    let codes = golden["error_codes"]
        .as_array()
        .expect("error_codes is a list");
    let golden_codes: Vec<String> = codes
        .iter()
        .map(|entry| entry["code"].as_str().expect("a code").to_owned())
        .collect();
    assert_eq!(
        golden_codes.iter().collect::<BTreeSet<_>>().len(),
        golden_codes.len(),
        "an error code is listed twice"
    );

    let document = lines(SPEC);
    let spec_codes = items(
        &at(&document, &["components", "schemas", "ErrorCode"]),
        "enum",
    )
    .expect("ErrorCode lists its enum");
    let mut spec_sorted = spec_codes.clone();
    spec_sorted.sort_unstable();
    let mut golden_sorted: Vec<&str> = golden_codes.iter().map(String::as_str).collect();
    golden_sorted.sort_unstable();
    assert_eq!(
        spec_sorted, golden_sorted,
        "the spec's ErrorCode enum is the golden's error codes"
    );

    for route in golden["routes"].as_array().expect("routes is a list") {
        let errors = texts(&route["errors"]);
        for code in &errors {
            assert!(
                golden_codes.contains(code),
                "{code} is not in the error vocabulary"
            );
        }
        assert!(
            errors
                .iter()
                .any(|code| code.as_str() == "zdr_feature_not_supported"),
            "{} does not refuse a ZDR key",
            route["operation_id"].as_str().unwrap_or("a route")
        );
    }

    for entry in codes {
        let code = entry["code"].as_str().expect("a code");
        let status = entry["http_status"].as_u64().expect("a status");
        assert!((400..=599).contains(&status), "{code} is an error status");
    }

    let zdr_code = golden["zdr"]["refusal_code"]
        .as_str()
        .expect("the ZDR refusal code");
    assert_eq!(zdr_code, zdr_refusal::code::FEATURE_NOT_SUPPORTED);
    let zdr_feature = golden["zdr"]["feature"]
        .as_str()
        .expect("the ZDR feature key");
    assert!(
        zdr_refusal::feature(zdr_feature).is_some(),
        "{zdr_feature} is a key in the ZDR refusal catalogue"
    );
    let zdr_entry = codes
        .iter()
        .find(|entry| entry["code"].as_str() == Some(zdr_code))
        .expect("the ZDR code is in the error vocabulary");
    assert_eq!(
        zdr_entry["http_status"].as_u64(),
        golden["zdr"]["http_status"].as_u64(),
        "the ZDR refusal has the status the golden states"
    );

    let entitlement: Value =
        serde_json::from_str(ENTITLEMENT_REFUSALS).expect("the entitlement golden is JSON");
    let entitlement_codes: Vec<&str> = entitlement["codes"]
        .as_array()
        .expect("the entitlement codes are a list")
        .iter()
        .filter_map(|row| row["code"].as_str())
        .collect();
    for entry in codes {
        if entry["shared_with"].as_str() == Some("model-entitlement-refusals.json") {
            let code = entry["code"].as_str().expect("a code");
            assert!(
                entitlement_codes.contains(&code),
                "{code} is in model-entitlement-refusals.json"
            );
        }
    }
}

#[test]
fn the_start_body_is_a_closed_allow_list_with_the_stated_limits() {
    let golden = golden();
    let document = lines(SPEC);
    let request = at(&document, &["components", "schemas", "StartRunRequest"]);
    assert_eq!(
        scalar(&request, "additionalProperties"),
        Some("false"),
        "the body refuses unknown fields"
    );

    let spec_fields = set_of(keys(&at(&request, &["properties"])));
    let start = golden["routes"]
        .as_array()
        .expect("routes is a list")
        .iter()
        .find(|route| route["operation_id"].as_str() == Some("startRun"))
        .expect("startRun is a golden route");
    assert_eq!(
        spec_fields,
        golden_set(&start["body_fields"]),
        "the body fields are the golden allow-list"
    );

    let refused = set_of(
        golden["refused_fields"]
            .as_object()
            .expect("refused_fields is an object")
            .keys()
            .map(String::as_str),
    );
    assert!(
        refused.is_disjoint(&golden_set(&start["body_fields"])),
        "a refused field is never an allowed one"
    );

    let limits = &golden["limits"];
    let input = at(&request, &["properties", "input"]);
    assert_eq!(
        scalar(&input, "maxLength").and_then(|value| value.parse::<u64>().ok()),
        limits["input_max_chars"].as_u64(),
        "the input cap"
    );
    let metadata = at(&request, &["properties", "metadata"]);
    assert_eq!(
        scalar(&metadata, "maxProperties").and_then(|value| value.parse::<u64>().ok()),
        limits["metadata_max_pairs"].as_u64(),
        "the metadata pair cap"
    );
}

#[test]
fn the_response_field_sets_are_the_golden_ones() {
    let golden = golden();
    let document = lines(SPEC);
    for schema in [
        "StartRunAccepted",
        "Run",
        "RunCancellation",
        "Message",
        "PageMeta",
        "PageLinks",
    ] {
        let properties = at(&document, &["components", "schemas", schema, "properties"]);
        assert_eq!(
            set_of(keys(&properties)),
            golden_set(&golden["response_fields"][schema]),
            "{schema} fields"
        );
    }
    let metadata = at(
        &document,
        &[
            "components",
            "schemas",
            "Run",
            "properties",
            "metadata",
            "properties",
        ],
    );
    assert_eq!(
        set_of(keys(&metadata)),
        golden_set(&golden["response_fields"]["Run.metadata"]),
        "Run.metadata fields"
    );
}

#[test]
fn every_details_schema_is_an_object() {
    let document = lines(SPEC);
    let mut seen = 0;
    for (index, line) in document.iter().enumerate() {
        if line.text == "details:" {
            seen += 1;
            let next = document.get(index + 1).expect("details has a schema");
            assert_eq!(next.text, "type: object", "details must be an object");
        }
    }
    assert!(seen > 0, "the spec declares a details schema");
}

#[test]
fn every_refused_field_names_a_refused_feature() {
    let golden = golden();
    let features = set_of(
        golden["refused_features"]
            .as_array()
            .expect("refused_features is a list")
            .iter()
            .filter_map(|feature| feature.as_str()),
    );
    for (field, feature) in golden["refused_fields"]
        .as_object()
        .expect("refused_fields is an object")
    {
        assert!(
            features.contains(feature.as_str().expect("a feature name")),
            "{field} names a feature that is not refused"
        );
    }
}

#[test]
fn the_golden_digest_is_the_pinned_one() {
    let normalised = GOLDEN.replace("\r\n", "\n");
    let digest: String = Sha256::digest(normalised.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let pinned = PIN.split_whitespace().next().expect("the pin names a digest");
    assert_eq!(
        digest, pinned,
        "agent-api-v1.json changed without its .sha256 pin being refreshed"
    );
}
