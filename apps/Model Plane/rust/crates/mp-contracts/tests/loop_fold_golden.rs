//! Golden for the durable loop fold (build plan WS03 S1): a checked-in v1 value
//! must keep parsing, a newer writer's extra field must not break an older
//! reader, another version is refused, and a credential-shaped string is never
//! persisted. The fixture is the contract: change it only with a version bump.

use mp_contracts::loop_fold::{assert_no_credentials, FoldRefusal, LoopFoldV1, PauseKind};

const GOLDEN: &str = include_str!("fixtures/loop-fold-v1.json");

#[test]
fn the_v1_golden_parses_and_round_trips() {
    let fold = LoopFoldV1::from_bytes(GOLDEN.as_bytes()).expect("v1 golden parses");
    assert_eq!(fold.run_id, "run-golden");
    assert_eq!(fold.messages.len(), 2);
    assert_eq!(
        fold.pending.as_ref().map(|p| p.kind),
        Some(PauseKind::Approval)
    );
    // Unknown extension keys are kept, not dropped (loop-guard state rides here).
    assert_eq!(fold.extensions["loop_guard"]["repeats"], 0);
    let again = LoopFoldV1::from_bytes(&fold.to_bytes().expect("serialize")).expect("parse again");
    assert_eq!(again, fold);
}

#[test]
fn an_unknown_top_level_field_is_ignored_and_another_version_is_refused() {
    let mut value: serde_json::Value = serde_json::from_str(GOLDEN).unwrap();
    value["a_later_field"] = serde_json::json!(true);
    assert!(LoopFoldV1::from_bytes(&serde_json::to_vec(&value).unwrap()).is_ok());
    value["v"] = serde_json::json!(2);
    assert_eq!(
        LoopFoldV1::from_bytes(&serde_json::to_vec(&value).unwrap()),
        Err(FoldRefusal::UnsupportedVersion(2))
    );
}

#[test]
fn a_bearer_shaped_string_fails_assert_no_credentials() {
    let mut fold = LoopFoldV1::from_bytes(GOLDEN.as_bytes()).unwrap();
    assert_eq!(assert_no_credentials(&fold), Ok(()));
    fold.messages[0].content = "curl -H 'Authorization: Bearer abc' https://x".to_owned();
    assert_eq!(
        assert_no_credentials(&fold),
        Err(FoldRefusal::ContainsCredential)
    );
}
