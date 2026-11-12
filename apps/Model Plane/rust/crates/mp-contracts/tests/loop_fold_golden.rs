//! Golden for the durable loop fold (build plan WS03 S1): a checked-in v1 value
//! must keep parsing, a newer writer's extra field must not break an older
//! reader, another version is refused, and a credential-shaped string is never
//! persisted. The fixture is the contract: change it only with a version bump.

use mp_contracts::loop_fold::{
    assert_no_credentials, child_ceiling_in, contains_credential_shape, park_ref, parks_in,
    prompt_digest, route_connection_in, route_model_in, FoldRefusal, LoopFoldV1, PauseKind,
    PendingPause, EXT_CHILD_CEILING, EXT_EFFECT_LEDGER, EXT_NOT_RESUMABLE, EXT_PARKS,
    EXT_ROUTE_CONNECTION, EXT_ROUTE_MODEL, LOOP_PARKING_ENV, MAX_PARKS_PER_RUN,
    PARK_DEFAULT_WAIT_SECS, PARK_SHORT_WAIT_SECS,
};

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

// ---- The writer's contract (build plan WS03 S6) ----
//
// execution-core writes three kinds of fold and a later build resumes from them.
// These three checked-in values pin the shape the writer must produce, so the
// resume is tested against a fixed contract instead of against whatever the writer
// happens to emit this week. One scenario, three moments of it:
//
// * `round`: the end of round 1 (a read call ran, the history holds its result).
// * `pre_effect`: round 2 has just been answered by the model with three calls
//   (a read, a call the loop rejects, and an effectful delete). Nothing has been
//   dispatched, but the rejected call already has its outcome and the fold carries it.
// * `pause`: the same round after the read ran and the rejection was recorded, with
//   the delete waiting on a human.
//
// execution-core's `runtime_loop::fold` tests build their fold from the loop's own
// locals and compare it with these same files.

const WRITER_ROUND: &str = include_str!("fixtures/loop-fold-v1-writer-round.json");
const WRITER_PRE_EFFECT: &str = include_str!("fixtures/loop-fold-v1-writer-pre-effect.json");
const WRITER_PAUSE: &str = include_str!("fixtures/loop-fold-v1-writer-pause.json");

/// The system prompt and the tool definitions the scenario's run was offered. The
/// golden's `prompt_digest` is computed from exactly these.
const GOLDEN_SYSTEM_PROMPT: &str = "SYSTEM_PROMPT_GOLDEN";
const GOLDEN_TOOLS: [(&str, &str, &str); 2] = [
    (
        "knowledge_search",
        "Search the org knowledge base.",
        r#"{"type":"object","properties":{"query":{"type":"string"}}}"#,
    ),
    (
        "delete_records",
        "Delete records (destructive).",
        r#"{"type":"object","properties":{}}"#,
    ),
];

fn parse(golden: &str) -> LoopFoldV1 {
    LoopFoldV1::from_bytes(golden.as_bytes()).expect("the writer golden parses")
}

fn as_value(golden: &str) -> serde_json::Value {
    serde_json::from_str(golden).expect("golden json")
}

#[test]
fn every_writer_golden_is_a_valid_v1_fold_that_round_trips_exactly() {
    for (name, golden) in [
        ("round", WRITER_ROUND),
        ("pre_effect", WRITER_PRE_EFFECT),
        ("pause", WRITER_PAUSE),
    ] {
        let fold = parse(golden);
        assert_eq!(fold.run_id, "run-golden-w", "{name}");
        assert_eq!(fold.segment_no, 1, "{name}: the first segment");
        assert!(fold.is_resumable(), "{name}");
        assert_eq!(assert_no_credentials(&fold), Ok(()), "{name}");
        // Re-serializing writes the same value: no field is dropped or invented, so
        // the shape the writer must produce is the whole of what the fixture says.
        let again = serde_json::to_value(&fold).expect("serialize");
        assert_eq!(again, as_value(golden), "{name}: the golden is the canonical form");
        let bytes = fold.to_bytes().expect("serialize");
        assert_eq!(LoopFoldV1::from_bytes(&bytes).expect("parse again"), fold, "{name}");
    }
}

#[test]
fn every_writer_fold_names_the_ledger_and_the_prompt_it_was_written_under() {
    for (name, golden) in [
        ("round", WRITER_ROUND),
        ("pre_effect", WRITER_PRE_EFFECT),
        ("pause", WRITER_PAUSE),
    ] {
        let fold = parse(golden);
        assert_eq!(
            fold.extensions.get(EXT_EFFECT_LEDGER),
            Some(&serde_json::json!(true)),
            "{name}: the writer requires the effect ledger and says so"
        );
        assert!(!fold.extensions.contains_key(EXT_NOT_RESUMABLE), "{name}");
        // The shared digest of the system prompt (the first message) and the tools.
        assert_eq!(fold.messages[0].role, "system", "{name}");
        assert_eq!(fold.messages[0].content, GOLDEN_SYSTEM_PROMPT, "{name}");
        assert_eq!(
            fold.prompt_digest,
            prompt_digest(&fold.messages[0].content, &GOLDEN_TOOLS),
            "{name}: prompt_digest is loop_fold::prompt_digest over the prompt and tools"
        );
        // The authority names exactly the offered tools, sorted.
        assert_eq!(
            fold.authority.tool_names,
            vec!["delete_records".to_owned(), "knowledge_search".to_owned()],
            "{name}"
        );
    }
}

#[test]
fn the_round_fold_holds_no_open_round_and_no_pause() {
    let fold = parse(WRITER_ROUND);
    assert!(fold.round_in_flight.is_none(), "a round is saved between rounds");
    assert!(fold.pending.is_none());
    assert_eq!(fold.rounds_executed, 1, "one round has run");
    assert_eq!(fold.step_seq, 1);
    assert!(fold.grounded);
    assert_eq!(fold.attempted_retrievals, vec!["hybrid|5|find the contract".to_owned()]);
}

#[test]
fn the_pre_effect_fold_lists_every_call_in_order_and_resolves_only_the_refused_one() {
    let fold = parse(WRITER_PRE_EFFECT);
    let round = fold.round_in_flight.as_ref().expect("the round is open");
    // rounds_executed counts rounds STARTED, including this open one.
    assert_eq!(fold.rounds_executed, 2);
    assert!(fold.pending.is_none(), "nothing waits on a human yet");
    // Nothing has been dispatched when it is saved, so the only calls with an outcome
    // are the ones the loop refused before dispatch (not offered, truncated, over a
    // rung or a leaf): they are resolved here, matched by call id, with no step. A
    // resume reads them as refused; it never re-derives a refusal, because some
    // (truncation) depend on state the fold does not carry.
    let resolved: Vec<(&str, &str)> = round
        .completed
        .iter()
        .map(|c| (c.call_id.as_str(), c.step_id.as_str()))
        .collect();
    assert_eq!(resolved, [("call-3", "")]);
    assert!(round.completed[0].output.is_empty() && !round.completed[0].error.is_empty());
    // Exactly the calls without a step are resolved, and no call with a step is.
    for call in &round.calls {
        assert_eq!(
            round.completed.iter().any(|c| c.call_id == call.id),
            call.step_id.is_empty(),
            "{}: resolved if and only if it was refused",
            call.id
        );
    }
    let names: Vec<&str> = round.calls.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["knowledge_search", "unknown_tool", "delete_records"], "the model's order");
    // A call rejected before dispatch has no step id: its id is what identifies it.
    let step_ids: Vec<&str> = round.calls.iter().map(|c| c.step_id.as_str()).collect();
    assert_eq!(step_ids, ["tool_2_call-2", "", "tool_3_call-4"]);
    assert!(round.calls.iter().all(|c| !c.id.is_empty()));
    // The step counter is past every id handed out (the rejected call took none).
    assert_eq!(fold.step_seq, 3);
}

#[test]
fn the_pause_fold_names_the_pause_by_the_approvals_idempotency_key() {
    let fold = parse(WRITER_PAUSE);
    let round = fold.round_in_flight.as_ref().expect("the round is open");
    let pending = fold.pending.as_ref().expect("the pause");
    assert_eq!(pending.kind, PauseKind::Approval);
    // `{run_id}:{step_id}`: the key the approval is created under, known before the
    // approval exists, which is what lets the fold be saved first.
    assert_eq!(pending.pause_ref, format!("{}:{}", fold.run_id, pending.step_id));
    assert_eq!(pending.call, "delete_records");
    assert_eq!(pending.action_fingerprint.len(), 64);
    // The paused call is in `calls` and is NOT in `completed`.
    assert!(round.calls.iter().any(|c| c.step_id == pending.step_id));
    assert!(round.completed.iter().all(|c| c.step_id != pending.step_id));
    // Every other call has an outcome, matched by call id; the rejected one has an
    // empty step id and an error.
    let by_call: Vec<(&str, &str)> = round
        .completed
        .iter()
        .map(|c| (c.call_id.as_str(), c.step_id.as_str()))
        .collect();
    assert_eq!(by_call, [("call-2", "tool_2_call-2"), ("call-3", "")]);
    let rejected = &round.completed[1];
    assert!(rejected.output.is_empty() && !rejected.error.is_empty());
    // Same counters as the pre_effect fold of the same round.
    let pre = parse(WRITER_PRE_EFFECT);
    assert_eq!(fold.rounds_executed, pre.rounds_executed);
    assert_eq!(fold.step_seq, pre.step_seq);
    assert_eq!(round.calls, pre.round_in_flight.as_ref().unwrap().calls);
    assert_eq!(fold.messages, pre.messages);
}

/// The folds of an OPEN round carry the retrievals attempted as they stood at the START
/// of the round, in the real `route|top_k|normalised query` format. The phase-1 pre-pass
/// inserts the round's own searches, so a set that held them would make a resume that
/// re-runs the pre-pass reject its own call as a duplicate of itself.
#[test]
fn the_open_round_folds_carry_the_retrievals_of_the_round_start_only() {
    for (name, golden) in [("pre_effect", WRITER_PRE_EFFECT), ("pause", WRITER_PAUSE)] {
        let fold = parse(golden);
        let round = fold.round_in_flight.as_ref().expect("the round is open");
        assert_eq!(fold.attempted_retrievals, vec!["hybrid|5|find the contract".to_owned()], "{name}");
        let mut searches = 0;
        for call in round
            .calls
            .iter()
            .filter(|call| !call.step_id.is_empty() && call.name == "knowledge_search")
        {
            let arguments: serde_json::Value =
                serde_json::from_str(&call.arguments_json).expect("arguments are JSON");
            let query = arguments["query"].as_str().expect("a query").to_lowercase();
            let signature = format!("hybrid|5|{query}");
            assert!(
                !fold.attempted_retrievals.contains(&signature),
                "{name}: {signature} is the open round's own search"
            );
            searches += 1;
        }
        assert_eq!(searches, 1, "{name}: the open round does search, so the check is not vacuous");
    }
}

#[test]
fn a_tombstone_round_trips_and_is_never_resumable() {
    let golden = parse(WRITER_ROUND);
    let tombstone = LoopFoldV1::tombstone(
        &golden.run_id,
        golden.segment_no,
        (golden.rounds_executed, golden.delegated_rounds, golden.step_seq),
        golden.authority.clone(),
        "oversize",
    );
    assert!(!tombstone.is_resumable());
    assert_eq!(tombstone.extensions[EXT_NOT_RESUMABLE], "oversize");
    let bytes = tombstone.to_bytes().expect("a tombstone serializes");
    let back = LoopFoldV1::from_bytes(&bytes).expect("a tombstone is a valid v1 fold");
    assert_eq!(back, tombstone);
    assert!(!back.is_resumable());
    assert_eq!(back.run_id, golden.run_id, "session-core checks the run id on save");
    assert_eq!(back.authority.digest(), golden.authority.digest());
}

#[test]
fn contains_credential_shape_agrees_with_assert_no_credentials() {
    let mut fold = parse(WRITER_ROUND);
    for (text, shaped) in [
        ("curl -H 'Authorization: Bearer abc' https://x", true),
        ("-----BEGIN CERTIFICATE-----", true),
        ("an ordinary message", false),
    ] {
        fold.messages[1].content = text.to_owned();
        assert_eq!(contains_credential_shape(text), shaped, "{text:?}");
        assert_eq!(assert_no_credentials(&fold).is_err(), shaped, "{text:?}");
    }
}

// ---- The park fold (build plan WS03 S8-A) ----
//
// A park is a fold written at a round boundary with `pending` of kind `park` and four
// extension keys that make it restorable without the original request: the count of
// parks the run has taken, the model and connection id of the pinned route, and the
// child ceiling. The keys are written ONLY on a park fold, and only identifiers and
// counts, never a credential. The writer (execution-core), the store validation
// (session-core) and the resume read them through the helpers pinned here, so a rename
// or a change of the reference's shape fails this test before it strands a live park.

/// The golden round fold, turned into the park fold the executor writes: the same
/// boundary, plus the park marker and the four keys.
fn park_fold(run_id: &str, parks: u32) -> LoopFoldV1 {
    let mut fold = parse(WRITER_ROUND);
    run_id.clone_into(&mut fold.run_id);
    fold.pending = Some(PendingPause {
        kind: PauseKind::Park,
        pause_ref: park_ref(run_id, parks),
        step_id: String::new(),
        call: String::new(),
        action_fingerprint: String::new(),
    });
    fold.extensions.insert(EXT_PARKS.to_owned(), serde_json::json!(parks));
    fold.extensions.insert(EXT_ROUTE_MODEL.to_owned(), serde_json::json!("model-golden"));
    fold.extensions
        .insert(EXT_ROUTE_CONNECTION.to_owned(), serde_json::json!("conn-golden"));
    fold.extensions.insert(EXT_CHILD_CEILING.to_owned(), serde_json::json!(2));
    fold
}

#[test]
fn park_extension_keys_and_park_ref_are_pinned() {
    // The wire names of the four keys, and of the switch that turns the writer on.
    assert_eq!(EXT_PARKS, "parks");
    assert_eq!(EXT_ROUTE_MODEL, "route_model");
    assert_eq!(EXT_ROUTE_CONNECTION, "route_connection_id");
    assert_eq!(EXT_CHILD_CEILING, "child_ceiling");
    assert_eq!(LOOP_PARKING_ENV, "EXECUTION_LOOP_PARKING_ENABLED");
    // The bounds the store enforces and the executor stays inside.
    assert_eq!(MAX_PARKS_PER_RUN, 5);
    assert_eq!(PARK_DEFAULT_WAIT_SECS, 300);
    assert_eq!(PARK_SHORT_WAIT_SECS, 60);
    // The reference shape: the executor writes it, session-core's save validates it and
    // the resume derives it again from the fold's own count.
    assert_eq!(park_ref("run-golden-w", 1), "park:run-golden-w:1");
    assert_eq!(park_ref("run-golden-w", MAX_PARKS_PER_RUN), "park:run-golden-w:5");

    // The committed round golden is a fold that never parked: none of the keys.
    let committed = parse(WRITER_ROUND);
    assert_eq!(parks_in(&committed), 0);
    assert_eq!(route_model_in(&committed), None);
    assert_eq!(route_connection_in(&committed), None);
    assert_eq!(child_ceiling_in(&committed), None);
    for key in [EXT_PARKS, EXT_ROUTE_MODEL, EXT_ROUTE_CONNECTION, EXT_CHILD_CEILING] {
        assert!(!committed.extensions.contains_key(key), "{key}");
    }

    // A park fold: the exact extension object the writer must produce, ledger marker
    // included, and nothing else added.
    let fold = park_fold("run-golden-w", 2);
    assert_eq!(
        serde_json::to_value(&fold.extensions).expect("serialize"),
        serde_json::json!({
            "effect_ledger": true,
            // The fold writer records the connector taint on every fold (EXT_CONNECTOR_TAINT). The round
            // golden carries it as false, and a park fold built from that golden keeps it.
            "connector_taint": false,
            "parks": 2,
            "route_model": "model-golden",
            "route_connection_id": "conn-golden",
            "child_ceiling": 2,
        })
    );
    assert_eq!(
        fold.pending.as_ref().map(|p| (p.kind, p.pause_ref.as_str())),
        Some((PauseKind::Park, "park:run-golden-w:2"))
    );
    assert!(fold.is_resumable());
    assert_eq!(fold.extensions.get(EXT_EFFECT_LEDGER), Some(&serde_json::json!(true)));
    assert_eq!(assert_no_credentials(&fold), Ok(()), "a park fold holds no credential");

    // Through the wire the readers still see the values, and the reference still equals
    // the one derived from the fold's own count.
    let back = LoopFoldV1::from_bytes(&fold.to_bytes().expect("a park fold serializes"))
        .expect("a park fold is a valid v1 fold");
    assert_eq!(back, fold);
    assert_eq!(parks_in(&back), 2);
    assert_eq!(route_model_in(&back), Some("model-golden"));
    assert_eq!(route_connection_in(&back), Some("conn-golden"));
    assert_eq!(child_ceiling_in(&back), Some(2));
    let pending = back.pending.as_ref().expect("a park names its pause");
    assert_eq!(pending.pause_ref, park_ref(&back.run_id, parks_in(&back)));

    // An older reader that does not know the keys keeps them: they ride in `extensions`.
    let mut value = serde_json::to_value(&fold).expect("serialize");
    value["a_later_field"] = serde_json::json!(true);
    let kept = LoopFoldV1::from_bytes(&serde_json::to_vec(&value).unwrap()).expect("parses");
    assert_eq!(parks_in(&kept), 2);
}
