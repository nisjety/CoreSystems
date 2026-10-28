//! The shared posture matrix, run through the real mp-contracts resolver.
//!
//! `fixtures/zdr-posture-matrix.json` is a byte-identical copy of
//! `apps/Model Plane/api/contracts/zdr-posture-matrix.json`. It is pinned beside it and checked
//! against the canonical copy by `scripts/tests/zdr-contract-sync.test.sh`. Its
//! `enforcement_rows` are what this crate answers. `org_posture_rows` are evaluated by the
//! org-posture readers, and `decision_rows` by Auth Core's decision function; neither is
//! evaluated here.
//!
//! The loader is strict on purpose. It rejects an unknown field at any level, a missing field,
//! a value outside the matrix's vocabulary, and a refusal code outside the Z-05a catalogue. The
//! negative tests at the end show that it does. A loader that accepted more would let a changed
//! golden pass without the resolver being asked about it.

use mp_contracts::agentic_subscription::PROVIDER_ID;
use mp_contracts::retention::{
    resolve_turn, ClaimsView, Resolution, RouteView, WaiverView, WAIVER_CONTRACT_VERSION,
};
use mp_contracts::zdr_refusal::{self, code};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

const MATRIX: &str = include_str!("fixtures/zdr-posture-matrix.json");
const AZURE: &str = "azure-openai";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Waiver {
    version: u32,
    provider: String,
    connection: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Bearer {
    zdr: bool,
    org_zdr: Option<bool>,
    waiver: Option<Waiver>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Route {
    provider: String,
    connection: Option<String>,
    explicit_zdr: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Case {
    id: String,
    bearers: Vec<Bearer>,
    route: Route,
    decision: String,
    code: String,
}

impl Waiver {
    fn view(&self) -> WaiverView<'_> {
        WaiverView {
            version: self.version,
            provider: &self.provider,
            connection: &self.connection,
        }
    }
}

impl Bearer {
    fn view(&self) -> ClaimsView<'_> {
        ClaimsView {
            zdr: self.zdr,
            org_zdr: self.org_zdr,
            waiver: self.waiver.as_ref().map(Waiver::view),
        }
    }
}

impl Route {
    fn view(&self) -> RouteView<'_> {
        RouteView {
            provider: &self.provider,
            connection: self.connection.as_deref(),
            explicit_zdr: self.explicit_zdr,
        }
    }
}

type Loaded<T> = Result<T, String>;

fn matrix_value(text: &str) -> Value {
    serde_json::from_str(text).expect("the vendored matrix is JSON")
}

fn object<'v>(value: &'v Value, path: &str) -> Loaded<&'v Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| format!("{path}: must be an object"))
}

fn array<'v>(value: &'v Value, path: &str) -> Loaded<&'v Vec<Value>> {
    value
        .as_array()
        .ok_or_else(|| format!("{path}: must be an array"))
}

fn field<'v>(map: &'v Map<String, Value>, key: &str, path: &str) -> Loaded<&'v Value> {
    map.get(key)
        .ok_or_else(|| format!("{path}: missing field `{key}`"))
}

/// Every key must be one the schema names, and every required key must be present.
fn exact_keys(
    map: &Map<String, Value>,
    path: &str,
    required: &[&str],
    optional: &[&str],
) -> Loaded<()> {
    if let Some(key) = map
        .keys()
        .find(|key| !required.contains(&key.as_str()) && !optional.contains(&key.as_str()))
    {
        return Err(format!("{path}: unknown field `{key}`"));
    }
    if let Some(key) = required.iter().find(|key| !map.contains_key(**key)) {
        return Err(format!("{path}: missing field `{key}`"));
    }
    Ok(())
}

fn text(map: &Map<String, Value>, key: &str, path: &str) -> Loaded<String> {
    field(map, key, path)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{path}.{key}: must be a string"))
}

fn nullable_text(map: &Map<String, Value>, key: &str, path: &str) -> Loaded<Option<String>> {
    match field(map, key, path)? {
        Value::Null => Ok(None),
        value => value
            .as_str()
            .map(|text| Some(text.to_owned()))
            .ok_or_else(|| format!("{path}.{key}: must be a string or null")),
    }
}

fn flag(map: &Map<String, Value>, key: &str, path: &str) -> Loaded<bool> {
    field(map, key, path)?
        .as_bool()
        .ok_or_else(|| format!("{path}.{key}: must be a boolean"))
}

fn nullable_flag(map: &Map<String, Value>, key: &str, path: &str) -> Loaded<Option<bool>> {
    match field(map, key, path)? {
        Value::Null => Ok(None),
        value => value
            .as_bool()
            .map(Some)
            .ok_or_else(|| format!("{path}.{key}: must be a boolean or null")),
    }
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn load_waiver(value: &Value, path: &str) -> Loaded<Waiver> {
    let map = object(value, path)?;
    exact_keys(map, path, &["version", "provider", "connection"], &[])?;
    let version = field(map, "version", path)?
        .as_u64()
        .and_then(|version| u32::try_from(version).ok())
        .ok_or_else(|| format!("{path}.version: must be a whole number"))?;
    Ok(Waiver {
        version,
        provider: text(map, "provider", path)?,
        connection: text(map, "connection", path)?,
    })
}

fn load_bearer(value: &Value, path: &str) -> Loaded<Bearer> {
    let map = object(value, path)?;
    exact_keys(map, path, &["zdr", "org_zdr", "waiver"], &[])?;
    let waiver = match field(map, "waiver", path)? {
        Value::Null => None,
        waiver => Some(load_waiver(waiver, &format!("{path}.waiver"))?),
    };
    Ok(Bearer {
        zdr: flag(map, "zdr", path)?,
        org_zdr: nullable_flag(map, "org_zdr", path)?,
        waiver,
    })
}

fn load_route(value: &Value, path: &str) -> Loaded<Route> {
    let map = object(value, path)?;
    exact_keys(map, path, &["provider", "connection", "explicit_zdr"], &[])?;
    Ok(Route {
        provider: text(map, "provider", path)?,
        connection: nullable_text(map, "connection", path)?,
        explicit_zdr: flag(map, "explicit_zdr", path)?,
    })
}

/// The decision and code of an expectation must be in the matrix's vocabulary, and must agree:
/// a refusal carries a catalogue code, and any other decision carries the reason code the
/// vocabulary maps to it.
fn check_outcome(
    vocabulary: &Map<String, Value>,
    decision: &str,
    outcome_code: &str,
    path: &str,
) -> Loaded<()> {
    let decisions = strings(field(vocabulary, "decisions", "vocabulary")?);
    if !decisions.iter().any(|known| known == decision) {
        return Err(format!(
            "{path}.decision: `{decision}` is not in the vocabulary"
        ));
    }
    if decision == "refuse" {
        let refusal_codes = strings(field(vocabulary, "refusal_codes", "vocabulary")?);
        if !refusal_codes.iter().any(|known| known == outcome_code)
            || zdr_refusal::entry(outcome_code).is_none()
        {
            return Err(format!(
                "{path}.code: `{outcome_code}` is not a refusal code of the catalogue"
            ));
        }
        return Ok(());
    }
    let reasons = object(
        field(vocabulary, "reason_codes", "vocabulary")?,
        "reason_codes",
    )?;
    match reasons.get(outcome_code).and_then(Value::as_str) {
        Some(mapped) if mapped == decision => Ok(()),
        _ => Err(format!(
            "{path}.code: `{outcome_code}` is not a reason code for `{decision}`"
        )),
    }
}

fn load_case(value: &Value, path: &str, vocabulary: &Map<String, Value>) -> Loaded<Case> {
    let row = object(value, path)?;
    exact_keys(
        row,
        path,
        &["id", "title", "tags", "inputs", "expect"],
        &["note", "known_limit"],
    )?;
    let id = text(row, "id", path)?;
    text(row, "title", path)?;
    for tag in array(field(row, "tags", path)?, &format!("{path}.tags"))? {
        if tag.as_str().is_none() {
            return Err(format!("{path}.tags: every tag must be a string"));
        }
    }

    let inputs_path = format!("{path}.inputs");
    let inputs = object(field(row, "inputs", path)?, &inputs_path)?;
    exact_keys(inputs, &inputs_path, &["bearers", "route"], &[])?;
    let bearers = array(
        field(inputs, "bearers", &inputs_path)?,
        &format!("{inputs_path}.bearers"),
    )?
    .iter()
    .enumerate()
    .map(|(index, bearer)| load_bearer(bearer, &format!("{inputs_path}.bearers[{index}]")))
    .collect::<Loaded<Vec<_>>>()?;
    let route = load_route(
        field(inputs, "route", &inputs_path)?,
        &format!("{inputs_path}.route"),
    )?;

    let expect_path = format!("{path}.expect");
    let expect = object(field(row, "expect", path)?, &expect_path)?;
    exact_keys(expect, &expect_path, &["decision", "code"], &[])?;
    let decision = text(expect, "decision", &expect_path)?;
    let outcome_code = text(expect, "code", &expect_path)?;
    check_outcome(vocabulary, &decision, &outcome_code, &expect_path)?;

    Ok(Case {
        id,
        bearers,
        route,
        decision,
        code: outcome_code,
    })
}

/// The enforcement rows of a matrix, loaded strictly. The top level is checked too: a golden
/// that grows a field the loader does not know is a golden nobody has read.
fn load_enforcement(matrix: &Value) -> Loaded<Vec<Case>> {
    let top = object(matrix, "matrix")?;
    exact_keys(
        top,
        "matrix",
        &[
            "contract",
            "version",
            "waiver_contract_version",
            "description",
            "policy",
            "vocabulary",
            "rule",
            "required_tags",
            "org_posture_rows",
            "decision_rows",
            "enforcement_rows",
        ],
        &[],
    )?;
    let vocabulary = object(field(top, "vocabulary", "matrix")?, "vocabulary")?;
    array(
        field(top, "enforcement_rows", "matrix")?,
        "enforcement_rows",
    )?
    .iter()
    .enumerate()
    .map(|(index, row)| load_case(row, &format!("enforcement_rows[{index}]"), vocabulary))
    .collect()
}

fn mutated(change: impl FnOnce(&mut Value)) -> Value {
    let mut matrix = matrix_value(MATRIX);
    change(&mut matrix);
    matrix
}

#[test]
fn the_real_resolver_answers_every_enforcement_row_of_the_shared_golden() {
    let matrix = matrix_value(MATRIX);
    let cases = load_enforcement(&matrix)
        .unwrap_or_else(|error| panic!("the golden does not load: {error}"));
    assert!(
        !cases.is_empty(),
        "the golden has no enforcement rows to run"
    );
    let ids: BTreeSet<&str> = cases.iter().map(|case| case.id.as_str()).collect();
    assert_eq!(ids.len(), cases.len(), "enforcement row ids must be unique");

    let mut mismatches = Vec::new();
    for case in &cases {
        let bearers: Vec<ClaimsView<'_>> = case.bearers.iter().map(Bearer::view).collect();
        let got = resolve_turn(&bearers, &case.route.view());
        if (got.decision(), got.code()) != (case.decision.as_str(), case.code.as_str()) {
            mismatches.push(format!(
                "{}: the golden expects {} / {}, the resolver answers {} / {}",
                case.id,
                case.decision,
                case.code,
                got.decision(),
                got.code()
            ));
        }
        let expected_class = if case.decision == "refuse" {
            None
        } else {
            matrix["vocabulary"]["retention_classes"][case.decision.as_str()].as_str()
        };
        if got.retention_class() != expected_class {
            mismatches.push(format!(
                "{}: retention class {:?}, the vocabulary says {:?}",
                case.id,
                got.retention_class(),
                expected_class
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} mismatches across the {} enforcement rows:\n{}",
        mismatches.len(),
        cases.len(),
        mismatches.join("\n")
    );
}

#[test]
fn the_policy_and_vocabulary_are_the_ones_the_resolver_implements() {
    let matrix = matrix_value(MATRIX);
    assert_eq!(
        matrix["waiver_contract_version"].as_u64(),
        Some(u64::from(WAIVER_CONTRACT_VERSION)),
        "the matrix pins a different waiver contract version"
    );
    assert_eq!(
        strings(&matrix["policy"]["waiver_providers"]),
        vec![PROVIDER_ID.to_owned()],
        "only the subscription route has a waiver route, and the resolver reads it from the same constant"
    );
    assert!(
        strings(&matrix["policy"]["zdr_capable_connection_providers"]).is_empty(),
        "the golden names a ZDR-capable connection provider, and the resolver has no path for one \
         yet: add that path and its rows in the same change"
    );
    assert_eq!(
        matrix["vocabulary"]["retention_classes"],
        json!({"zdr": "zdr", "persistent": "persistent", "waived": "zdr_waived"}),
    );
    assert_eq!(
        strings(&matrix["vocabulary"]["decisions"]),
        vec!["zdr", "persistent", "waived", "refuse"],
    );
    for refusal in strings(&matrix["vocabulary"]["refusal_codes"]) {
        assert!(
            zdr_refusal::entry(&refusal).is_some(),
            "{refusal} is not in the Z-05a catalogue"
        );
    }
    let tags: BTreeSet<String> = ["org_posture_rows", "decision_rows", "enforcement_rows"]
        .iter()
        .flat_map(|table| matrix[*table].as_array().into_iter().flatten())
        .flat_map(|row| strings(&row["tags"]))
        .collect();
    for required in strings(&matrix["required_tags"]) {
        assert!(
            tags.contains(&required),
            "no row carries the required scenario `{required}`"
        );
    }
}

#[test]
fn the_loader_rejects_an_unknown_field_at_every_level() {
    let rejected = [
        (
            "the top level",
            mutated(|m| {
                m["surprise"] = json!(1);
            }),
        ),
        (
            "a row",
            mutated(|m| {
                m["enforcement_rows"][0]["surprise"] = json!(1);
            }),
        ),
        (
            "the inputs",
            mutated(|m| {
                m["enforcement_rows"][0]["inputs"]["surprise"] = json!(1);
            }),
        ),
        (
            "a bearer",
            mutated(|m| {
                m["enforcement_rows"][0]["inputs"]["bearers"][0]["surprise"] = json!(1);
            }),
        ),
        (
            "a waiver",
            mutated(|m| {
                m["enforcement_rows"][0]["inputs"]["bearers"][0]["waiver"] = json!({
                    "version": 1, "provider": PROVIDER_ID, "connection": "conn-1", "surprise": 1
                });
            }),
        ),
        (
            "the route",
            mutated(|m| {
                m["enforcement_rows"][0]["inputs"]["route"]["surprise"] = json!(1);
            }),
        ),
        (
            "the expectation",
            mutated(|m| {
                m["enforcement_rows"][0]["expect"]["surprise"] = json!(1);
            }),
        ),
    ];
    for (level, matrix) in rejected {
        let error = load_enforcement(&matrix)
            .expect_err(&format!("an unknown field at {level} must be rejected"));
        assert!(
            error.contains("unknown field `surprise`"),
            "{level}: wrong error: {error}"
        );
    }
}

#[test]
fn the_loader_rejects_a_row_without_a_code_and_an_answer_outside_the_vocabulary() {
    let without_code = mutated(|m| {
        m["enforcement_rows"][0]["expect"]
            .as_object_mut()
            .expect("expect is an object")
            .remove("code");
    });
    let error = load_enforcement(&without_code).expect_err("a row without a code is rejected");
    assert!(
        error.contains("missing field `code`"),
        "wrong error: {error}"
    );

    let unknown_decision = mutated(|m| {
        m["enforcement_rows"][0]["expect"]["decision"] = json!("maybe");
    });
    let error = load_enforcement(&unknown_decision).expect_err("an unknown decision is rejected");
    assert!(
        error.contains("`maybe` is not in the vocabulary"),
        "wrong error: {error}"
    );

    let unknown_refusal = mutated(|m| {
        let row = m["enforcement_rows"]
            .as_array_mut()
            .expect("rows are an array")
            .iter_mut()
            .find(|row| row["expect"]["decision"] == "refuse")
            .expect("the golden has a refusal row");
        row["expect"]["code"] = json!("zdr_made_up_code");
    });
    let error =
        load_enforcement(&unknown_refusal).expect_err("a code outside the catalogue is rejected");
    assert!(
        error.contains("not a refusal code of the catalogue"),
        "wrong error: {error}"
    );

    let reason_for_wrong_decision = mutated(|m| {
        m["enforcement_rows"][0]["expect"]["decision"] = json!("waived");
    });
    let error = load_enforcement(&reason_for_wrong_decision)
        .expect_err("a reason code that does not belong to its decision is rejected");
    assert!(
        error.contains("is not a reason code for `waived`"),
        "wrong error: {error}"
    );

    let wrong_type = mutated(|m| {
        m["enforcement_rows"][0]["inputs"]["route"]["explicit_zdr"] = json!("no");
    });
    let error =
        load_enforcement(&wrong_type).expect_err("a string where a flag belongs is rejected");
    assert!(error.contains("must be a boolean"), "wrong error: {error}");
}

/// The waivers the domain covers: none, the bound waiver, another connection, another provider,
/// another contract version, and an empty connection id.
fn waiver_domain() -> Vec<Option<WaiverView<'static>>> {
    vec![
        None,
        Some(WaiverView {
            version: 1,
            provider: PROVIDER_ID,
            connection: "conn-1",
        }),
        Some(WaiverView {
            version: 1,
            provider: PROVIDER_ID,
            connection: "conn-2",
        }),
        Some(WaiverView {
            version: 1,
            provider: AZURE,
            connection: "conn-1",
        }),
        Some(WaiverView {
            version: 2,
            provider: PROVIDER_ID,
            connection: "conn-1",
        }),
        Some(WaiverView {
            version: 1,
            provider: PROVIDER_ID,
            connection: "",
        }),
    ]
}

fn claim_domain() -> Vec<ClaimsView<'static>> {
    let mut claims = Vec::new();
    for zdr in [false, true] {
        for org_zdr in [None, Some(false), Some(true)] {
            for waiver in waiver_domain() {
                claims.push(ClaimsView {
                    zdr,
                    org_zdr,
                    waiver,
                });
            }
        }
    }
    claims
}

/// Every bearer set of up to two bearers over the claim domain, and the empty set.
fn bearer_domain() -> Vec<Vec<ClaimsView<'static>>> {
    let claims = claim_domain();
    let mut sets = vec![Vec::new()];
    sets.extend(claims.iter().map(|claim| vec![*claim]));
    for first in &claims {
        for second in &claims {
            sets.push(vec![*first, *second]);
        }
    }
    sets
}

fn route_domain() -> Vec<RouteView<'static>> {
    let mut routes = Vec::new();
    for provider in ["", PROVIDER_ID, AZURE] {
        for connection in [None, Some("conn-1"), Some("conn-2"), Some("")] {
            for explicit_zdr in [false, true] {
                routes.push(RouteView {
                    provider,
                    connection,
                    explicit_zdr,
                });
            }
        }
    }
    routes
}

/// The safety properties no answer may break, whatever the claims and the route. Returns the
/// names of the properties the answer breaks, so a failure says which promise was broken.
fn violations(
    bearers: &[ClaimsView<'_>],
    route: &RouteView<'_>,
    got: &Resolution<'_>,
) -> Vec<&'static str> {
    let mut broken = Vec::new();
    let agreed = bearers
        .windows(2)
        .all(|pair| pair[0].zdr == pair[1].zdr && pair[0].waiver == pair[1].waiver);
    let any_zdr = bearers.iter().any(|bearer| bearer.zdr);
    let any_waiver = bearers.iter().any(|bearer| bearer.waiver.is_some());
    let org_is_zdr = bearers.iter().any(|bearer| bearer.org_zdr == Some(true));
    let refused_as_mismatch = matches!(
        got,
        Resolution::Refuse(refusal) if refusal.code() == code::POSTURE_MISMATCH
    );

    if (bearers.is_empty() || !agreed) && !refused_as_mismatch {
        broken.push("a request without one agreed posture is refused as a posture mismatch");
    }
    if any_zdr && matches!(got, Resolution::Persistent { .. }) {
        broken.push("a ZDR token is never answered persistent");
    }
    if matches!(got, Resolution::Persistent { .. })
        && (any_waiver || org_is_zdr || any_zdr || route.explicit_zdr)
    {
        broken.push("persistent is answered only when no waiver, ZDR org, ZDR token or explicit request is present");
    }
    if let Resolution::Waived {
        provider,
        connection,
        ..
    } = *got
    {
        let bound = WaiverView {
            version: WAIVER_CONTRACT_VERSION,
            provider,
            connection,
        };
        let every_bearer_bound = bearers.iter().all(|bearer| {
            !bearer.zdr && bearer.org_zdr == Some(true) && bearer.waiver == Some(bound)
        });
        if !every_bearer_bound
            || route.explicit_zdr
            || provider != PROVIDER_ID
            || route.provider != provider
            || route.connection != Some(connection)
            || connection.is_empty()
        {
            broken.push("a waived answer names only the waiver the bearers carry, on the route it binds, with no explicit request");
        }
    }
    if let Some(waiver) = bearers.first().and_then(|bearer| bearer.waiver) {
        let route_matches = agreed
            && route.provider == waiver.provider
            && route.connection == Some(waiver.connection);
        if agreed && !route_matches && !matches!(got, Resolution::Refuse(_)) {
            broken.push("a waiver for another provider or connection is refused, never served");
        }
    }
    if matches!(got, Resolution::Zdr { .. }) {
        if route.is_connection_route() {
            broken.push("ZDR never answers a connection or subscription route");
        }
        if !(any_zdr || route.explicit_zdr) {
            broken.push("ZDR needs a ZDR token or an explicit request");
        }
        if any_waiver {
            broken.push("a waiver never yields ZDR");
        }
    }
    if route.explicit_zdr
        && matches!(
            got,
            Resolution::Waived { .. } | Resolution::Persistent { .. }
        )
    {
        broken.push("an explicit request is never answered persistent or waived");
    }
    broken
}

#[test]
fn no_combination_of_claims_and_route_breaks_a_safety_property() {
    let bearer_sets = bearer_domain();
    let routes = route_domain();
    let mut broken = Vec::new();
    let mut checked = 0_usize;
    for bearers in &bearer_sets {
        for route in &routes {
            let got = resolve_turn(bearers, route);
            for property in violations(bearers, route, &got) {
                broken.push(format!(
                    "{property}: bearers {bearers:?}, route {route:?}, answered {got:?}"
                ));
            }
            checked += 1;
        }
    }
    assert_eq!(checked, bearer_sets.len() * routes.len());
    assert!(
        broken.is_empty(),
        "{} safety violations across {checked} combinations, first: {}",
        broken.len(),
        broken.first().map_or("", String::as_str)
    );
}
