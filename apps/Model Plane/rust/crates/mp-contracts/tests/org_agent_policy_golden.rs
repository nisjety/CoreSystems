//! Golden cases of the org agent policy key schema (build plan WS25-S7).
//!
//! `api/contracts/org-agent-policy-schema.json` is the closed schema written out as
//! data. The table the registry validates against and the table the golden names must
//! agree key by key, in order, with the same kinds, orders, defaults and status, so a
//! key added to one and not the other fails here.

use mp_contracts::org_agent_policy::{defaults, loosest_ceiling, validate, Refusal, SETTINGS};
use serde_json::{Map, Value};

const GOLDEN: &str = include_str!("../../../../api/contracts/org-agent-policy-schema.json");

fn golden() -> Value {
    serde_json::from_str(GOLDEN).expect("the golden is JSON")
}

#[test]
fn org_agent_policy_schema_matches_the_golden_key_by_key() {
    let rows = golden()["settings"]
        .as_array()
        .cloned()
        .expect("settings is a list");
    assert_eq!(
        rows.len(),
        SETTINGS.len(),
        "a key was added to one side only"
    );
    for (row, def) in rows.iter().zip(SETTINGS) {
        assert_eq!(row["key"], def.key);
        assert_eq!(row["kind"], def.kind.as_str(), "kind of {}", def.key);
        let order: Vec<&str> = row["strictness_order"]
            .as_array()
            .expect("order is a list")
            .iter()
            .map(|token| token.as_str().expect("a token is text"))
            .collect();
        assert_eq!(order, def.strictness_order, "order of {}", def.key);
        assert_eq!(row["default"], def.default, "default of {}", def.key);
        assert_eq!(
            row["fail_closed_value"], def.fail_closed_value,
            "fail-closed of {}",
            def.key
        );
        assert_eq!(row["status"], def.status.as_str(), "status of {}", def.key);
    }
}

#[test]
fn org_agent_policy_refusal_vocabulary_matches_the_golden() {
    let golden_codes: Vec<String> = golden()["refusals"]
        .as_array()
        .expect("refusals is a list")
        .iter()
        .map(|code| code.as_str().expect("a code is text").to_owned())
        .collect();
    let enum_codes: Vec<String> = Refusal::ALL.iter().map(|r| r.code().to_owned()).collect();
    assert_eq!(golden_codes, enum_codes);
}

#[test]
fn org_agent_policy_golden_defaults_are_the_documented_document() {
    let settings = defaults();
    for row in golden()["settings"].as_array().expect("settings is a list") {
        let key = row["key"].as_str().expect("key is text");
        let default = row["default"].as_str().expect("default is text");
        let value = &settings[key];
        let in_force = match value {
            Value::String(text) => text.as_str(),
            Value::Bool(flag) => {
                if *flag {
                    "true"
                } else {
                    "false"
                }
            }
            Value::Object(object) => object["mode"].as_str().expect("mode is text"),
            other => panic!("unexpected value for {key}: {other}"),
        };
        assert_eq!(in_force, default, "default of {key}");
    }
    let accepted: Map<String, Value> = Map::new();
    assert_eq!(validate(&accepted, &loosest_ceiling()), Ok(settings));
}
