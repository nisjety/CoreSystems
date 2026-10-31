//! The model entitlement matrix (WS10-S3), run through the real matcher.
//!
//! `fixtures/model-entitlement-matrix.json` is a byte-identical copy of
//! `apps/Model Plane/api/contracts/model-entitlement-matrix.json`, which org-core
//! vendors too. Every row that names `mp-contracts` as an evaluator is answered
//! here by the function inference-core calls: `permit` rows by
//! `Policy::permits`, `route` rows by `ResolvedAccess::route`, `availability`
//! rows by `after_failed_refresh`, `refusal` rows by `Refusal`, and the
//! `resolve` and `shape` rows by reading the bodies org-core writes.
//!
//! The loader is strict: a row of a kind or with a field this file does not
//! know fails the test, so a changed golden cannot pass without the matcher
//! being asked about it.

use std::collections::BTreeSet;
use std::time::Duration;

use mp_contracts::model_entitlement::{
    after_failed_refresh, code, Availability, Candidate, LastAnswer, Policy, Reason,
    Refusal, ResolvedAccess, State, EFFORT_LADDER,
};
use serde_json::Value;

const MATRIX: &str = include_str!("fixtures/model-entitlement-matrix.json");
const CANONICAL: &str = include_str!("../../../../api/contracts/model-entitlement-matrix.json");

fn matrix() -> Value {
    serde_json::from_str(MATRIX).expect("the matrix is JSON")
}

fn rows_for_us(matrix: &Value) -> Vec<&Value> {
    matrix["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .filter(|row| {
            row["evaluated_by"]
                .as_array()
                .expect("evaluated_by")
                .iter()
                .any(|who| who == "mp-contracts")
        })
        .collect()
}

fn id(row: &Value) -> &str {
    row["id"].as_str().expect("a row id")
}

fn sorted_keys(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> = value
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

fn policy_of(value: &Value) -> Policy {
    Policy::from_json(value).unwrap_or_else(|| panic!("a policy the matcher reads: {value}"))
}

fn reason_of(name: &str) -> Option<Reason> {
    (!name.is_empty()).then(|| Reason::parse(name).unwrap_or_else(|| panic!("reason {name}")))
}

#[test]
fn the_fixture_is_the_canonical_matrix() {
    assert_eq!(
        MATRIX, CANONICAL,
        "tests/fixtures/model-entitlement-matrix.json must stay a byte-identical copy of \
         api/contracts/model-entitlement-matrix.json"
    );
}

#[test]
fn the_vocabulary_is_the_matchers() {
    let matrix = matrix();
    let names = |key: &str| -> BTreeSet<String> {
        matrix["vocabulary"][key]
            .as_array()
            .expect(key)
            .iter()
            .map(|name| name.as_str().expect("a name").to_owned())
            .collect()
    };
    let ours: BTreeSet<String> = Reason::ALL.iter().map(|r| r.as_str().to_owned()).collect();
    assert_eq!(names("reasons"), ours);
    let states: BTreeSet<String> = [State::None, State::Org, State::Role, State::MemberNotFound]
        .iter()
        .map(|s| s.as_str().to_owned())
        .collect();
    assert_eq!(names("states"), states);
    // Every row kind in the matrix is one this file evaluates or one org-core alone does.
    let kinds = names("kinds");
    for row in matrix["rows"].as_array().expect("rows") {
        assert!(kinds.contains(row["kind"].as_str().expect("kind")), "{}", id(row));
    }
}

#[test]
fn the_required_rows_are_present_and_ours() {
    let matrix = matrix();
    let ours: BTreeSet<&str> = rows_for_us(&matrix).into_iter().map(id).collect();
    for required in matrix["vocabulary"]["required_rows"]
        .as_array()
        .expect("required_rows")
    {
        let required = required.as_str().expect("a row id");
        assert!(ours.contains(required), "{required} is not evaluated here");
    }
}

#[test]
fn permit_rows() {
    let matrix = matrix();
    let mut ran = 0;
    for row in rows_for_us(&matrix).into_iter().filter(|r| r["kind"] == "permit") {
        ran += 1;
        let input = &row["input"];
        let policy = policy_of(&input["policy"]);
        let got = policy.permits(
            input["model"].as_str().expect("model"),
            input["provider"].as_str().expect("provider"),
        );
        let expect = &row["expect"];
        let want_ok = expect["permitted"].as_bool().expect("permitted");
        assert_eq!(got.is_ok(), want_ok, "{}", id(row));
        assert_eq!(
            got.err(),
            reason_of(expect["reason"].as_str().expect("reason")),
            "{}",
            id(row)
        );
    }
    assert!(ran >= 9, "the permit rows ran: {ran}");
}

#[test]
fn route_rows() {
    let matrix = matrix();
    let mut ran = 0;
    for row in rows_for_us(&matrix).into_iter().filter(|r| r["kind"] == "route") {
        ran += 1;
        let input = &row["input"];
        let access = ResolvedAccess {
            state: State::Org,
            revision: 1,
            role: String::new(),
            can_manage: false,
            policy: Some(policy_of(&input["policy"])),
        };
        let candidates: Vec<Candidate> = input["candidates"]
            .as_array()
            .expect("candidates")
            .iter()
            .map(|c| Candidate {
                model: c["model"].as_str().expect("model").to_owned(),
                provider: c["provider"].as_str().expect("provider").to_owned(),
                attested: c["attested"].as_bool().expect("attested"),
            })
            .collect();
        let route = access.route(
            &candidates,
            input["zdr"].as_bool().expect("zdr"),
            input["pinned"].as_bool().expect("pinned"),
        );
        let expect = &row["expect"];
        let want: Vec<&str> = expect["entitled"]
            .as_array()
            .expect("entitled")
            .iter()
            .map(|m| m.as_str().expect("a model"))
            .collect();
        assert_eq!(route.entitled, want, "{}", id(row));
        assert_eq!(
            route.reason,
            reason_of(expect["reason"].as_str().expect("reason")),
            "{}",
            id(row)
        );
    }
    assert!(ran >= 4, "the route rows ran: {ran}");
}

#[test]
fn availability_rows() {
    let matrix = matrix();
    let mut ran = 0;
    for row in rows_for_us(&matrix)
        .into_iter()
        .filter(|r| r["kind"] == "availability")
    {
        ran += 1;
        let input = &row["input"];
        assert_eq!(input["fetch"], "failed", "{}", id(row));
        let last = match &input["last_answer"] {
            Value::Null => None,
            answer => Some(LastAnswer {
                restricted: answer["restricted"].as_bool().expect("restricted"),
                age: Duration::from_secs(answer["age_seconds"].as_u64().expect("age_seconds")),
            }),
        };
        let expect = &row["expect"];
        match after_failed_refresh(last) {
            Availability::UseLastAnswer => {
                assert_eq!(expect["decision"], "use_last_answer", "{}", id(row));
            }
            Availability::Refuse => {
                assert_eq!(expect["decision"], "refuse", "{}", id(row));
                let refusal = Refusal::unavailable();
                assert_eq!(refusal.code(), expect["code"].as_str().expect("code"));
                assert_eq!(
                    u64::from(refusal.http_status()),
                    expect["http_status"].as_u64().expect("http_status")
                );
                assert_eq!(refusal.retryable(), expect["retryable"]);
                assert_eq!(refusal.code(), code::ENTITLEMENT_UNAVAILABLE);
            }
        }
    }
    assert!(ran >= 4, "the availability rows ran: {ran}");
}

#[test]
fn refusal_rows() {
    let matrix = matrix();
    let mut ran = 0;
    for row in rows_for_us(&matrix).into_iter().filter(|r| r["kind"] == "refusal") {
        ran += 1;
        let input = &row["input"];
        let refusal = Refusal::not_entitled(
            input["model"].as_str().expect("model"),
            reason_of(input["reason"].as_str().expect("reason")).expect("a reason"),
        );
        let body = refusal.json_body();
        let expect = &row["expect"];
        assert_eq!(
            u64::from(refusal.http_status()),
            expect["http_status"].as_u64().expect("http_status")
        );
        assert_eq!(body["error"]["code"], expect["code"], "{}", id(row));
        assert_eq!(
            Value::from(sorted_keys(&body["error"])),
            expect["body_keys"],
            "{}",
            id(row)
        );
        assert_eq!(
            Value::from(sorted_keys(&body["error"]["details"])),
            expect["details_keys"],
            "{}",
            id(row)
        );
    }
    assert!(ran >= 1, "the refusal rows ran: {ran}");
}

#[test]
fn resolve_rows_are_read_the_same_whatever_the_zdr_posture() {
    // org-core resolves; the matcher reads what it wrote. The posture is not an
    // input of either: the same body permits the same models under both.
    let matrix = matrix();
    let mut ran = 0;
    for row in rows_for_us(&matrix)
        .into_iter()
        .filter(|r| r["kind"] == "resolve")
    {
        ran += 1;
        let expect = &row["expect"];
        let access = ResolvedAccess::from_body(expect)
            .unwrap_or_else(|| panic!("the matcher reads the body of {}", id(row)));
        assert_eq!(access.state.as_str(), expect["state"], "{}", id(row));
        assert_eq!(access.revision, expect["revision"].as_i64().unwrap());
        assert_eq!(access.role, expect["role"].as_str().unwrap());
        assert_eq!(access.can_manage, expect["can_manage"].as_bool().unwrap());
        // Neither the resolution nor the matcher takes the posture as an input,
        // so the row's two postures are one answer; the row must list both.
        assert_eq!(
            row["input"]["zdr"],
            serde_json::json!([false, true]),
            "{}",
            id(row)
        );
    }
    assert_eq!(ran, 1, "one resolve row names mp-contracts");
}

#[test]
fn shape_rows_hold_no_content() {
    let matrix = matrix();
    let all_rows = matrix["rows"].as_array().expect("rows");
    let mut ran = 0;
    for row in rows_for_us(&matrix).into_iter().filter(|r| r["kind"] == "shape") {
        ran += 1;
        let expect = &row["expect"];
        let want_keys = Value::from(
            expect["keys"]
                .as_array()
                .expect("keys")
                .iter()
                .map(|k| k.as_str().unwrap().to_owned())
                .collect::<Vec<_>>(),
        );
        match row["input"]["subject"].as_str().expect("subject") {
            "policy" => {
                // Every policy any row writes has exactly these keys, and the
                // matcher reads each of them.
                for other in all_rows {
                    let policies = [
                        &other["expect"]["policy"],
                        &other["input"]["policy"],
                        &other["input"]["set"]["org_default"],
                    ];
                    for policy in policies.into_iter().filter(|p| p.is_object()) {
                        assert!(
                            sorted_keys(policy).iter().all(|k| expect["keys"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .any(|allowed| allowed == k.as_str())),
                            "{}: a policy holds only {want_keys}",
                            id(other)
                        );
                        assert!(Policy::from_json(policy).is_some(), "{}", id(other));
                    }
                }
            }
            "resolution" => {
                for other in all_rows.iter().filter(|r| r["kind"] == "resolve") {
                    assert_eq!(
                        Value::from(sorted_keys(&other["expect"])),
                        want_keys,
                        "{}",
                        id(other)
                    );
                }
            }
            other => panic!("unknown shape subject {other}"),
        }
    }
    assert_eq!(ran, 2, "both shape rows run here");
}

#[test]
fn the_effort_ladder_is_the_canonical_one() {
    assert_eq!(
        EFFORT_LADDER,
        ["minimal", "low", "medium", "high", "xhigh", "max"]
    );
}

#[test]
fn a_body_the_matcher_cannot_read_is_never_unrestricted() {
    for body in [
        serde_json::json!({"state": "weird", "policy": null}),
        serde_json::json!({"state": "org", "policy": null}),
        serde_json::json!({"state": "role", "policy": {"allow": "gpt-5"}}),
        serde_json::json!({"policy": null}),
    ] {
        assert!(ResolvedAccess::from_body(&body).is_none(), "{body}");
    }
    assert!(ResolvedAccess::from_body(&serde_json::json!({"state": "none", "policy": null}))
        .is_some_and(|access| !access.restricted()));
}

#[test]
fn a_member_who_is_not_found_gets_its_own_reason_and_nothing_else() {
    let access = ResolvedAccess::from_body(&serde_json::json!({
        "state": "member_not_found",
        "revision": 3,
        "role": "",
        "can_manage": false,
        "policy": {"allow_mode": "listed", "allow": [], "deny": []},
        "sources": [],
    }))
    .expect("readable");
    assert!(access.restricted());
    assert_eq!(
        access.permits("gpt-5", "openai"),
        Err(Reason::MemberNotFound)
    );
}

#[test]
fn an_effort_above_the_ceiling_is_brought_down_and_one_below_is_kept() {
    let policy = Policy {
        max_effort: "medium".to_owned(),
        allow_all: true,
        ..Policy::default()
    };
    assert_eq!(policy.clamp_effort("high"), "medium");
    assert_eq!(policy.clamp_effort("MAX"), "medium");
    assert_eq!(policy.clamp_effort("low"), "low");
    assert_eq!(policy.clamp_effort("medium"), "medium");
    assert_eq!(policy.clamp_effort("unheard-of"), "medium");
    assert_eq!(policy.clamp_effort(""), "");
    let open = Policy::default();
    assert_eq!(open.clamp_effort("max"), "max");
}

#[test]
fn the_stale_bounds_are_inclusive() {
    let at = |restricted, secs| {
        after_failed_refresh(Some(LastAnswer {
            restricted,
            age: Duration::from_secs(secs),
        }))
    };
    assert_eq!(at(true, 900), Availability::UseLastAnswer);
    assert_eq!(at(true, 901), Availability::Refuse);
    assert_eq!(at(false, 120), Availability::UseLastAnswer);
    assert_eq!(at(false, 121), Availability::Refuse);
    assert_eq!(after_failed_refresh(None), Availability::Refuse);
}
