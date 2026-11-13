//! EP-7 S5 [W3-EP-7-S5-a]: a stopped goal becomes one waiting item, a test-only child module of
//! `attention_events`. Pure: the goal item build, its envelope (six keys, ids and closed tokens
//! only), and that a pending row keeps its four keys. The four-key golden test of a pending row is
//! `tests::golden_run_attention_wire_body`, kept unchanged.

use super::*;
use chrono::TimeZone;

fn at() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 12, 9, 0, 0).unwrap()
}

fn stopped(status: &str, latest_run_status: Option<&str>) -> StoppedGoalRow {
    StoppedGoalRow {
        org_id: "org-1".to_owned(),
        user_id: "u1".to_owned(),
        thread_id: "thread-1".to_owned(),
        run_id: "run-1".to_owned(),
        source_id: "goal_1".to_owned(),
        status: status.to_owned(),
        since: at(),
        latest_run_status: latest_run_status.map(str::to_owned),
        latest_run_at: Some(at() - chrono::Duration::minutes(1)),
        retention_class: "persistent".to_owned(),
        zdr: false,
    }
}

#[test]
fn a_stopped_goal_raises_the_state_its_status_names() {
    assert_eq!(
        goal_stop(&stopped("met", Some("completed"))),
        Some((AttentionState::Completed, AttentionReason::Goal))
    );
    for status in ["budget_reached", "turns_reached", "blocked"] {
        assert_eq!(
            goal_stop(&stopped(status, Some("completed"))),
            Some((AttentionState::NeedsInput, AttentionReason::Goal)),
            "{status}"
        );
    }
    for status in ["active", "paused", "cleared", ""] {
        assert_eq!(
            goal_stop(&stopped(status, Some("completed"))),
            None,
            "{status}"
        );
    }
}

#[test]
fn a_goal_is_not_raised_while_its_threads_run_is_working() {
    assert_eq!(goal_stop(&stopped("blocked", Some("running"))), None);
    assert_eq!(goal_stop(&stopped("met", Some("running"))), None);
}

fn goal_item(status: &str, state: AttentionState) -> Waiting {
    Waiting {
        org_id: "org-1".to_owned(),
        user_id: "u1".to_owned(),
        thread_id: "thread-1".to_owned(),
        run_id: "run-1".to_owned(),
        state,
        source_id: "goal_1".to_owned(),
        goal: Some(GoalStop {
            reason: AttentionReason::Goal,
            status: status.to_owned(),
        }),
    }
}

#[test]
fn a_goal_item_names_its_reason_and_closed_status_and_nothing_else() {
    let envelope = build_run_attention_envelope(
        &goal_item("budget_reached", AttentionState::NeedsInput),
        at(),
    );
    assert!(envelope.validate().is_ok());
    assert!(!envelope.zdr);
    assert_eq!(
        envelope.causation_id, "goal_1",
        "the goal is what caused the event"
    );
    assert_eq!(
        envelope.payload,
        serde_json::json!({
            "goal_status": "budget_reached",
            "reason": "ATTENTION_REASON_GOAL",
            "run_id": "run-1",
            "source_id": "goal_1",
            "state": "ATTENTION_STATE_NEEDS_INPUT",
            "thread_id": "thread-1",
        })
    );
}

#[test]
fn a_pending_row_keeps_its_four_keys_beside_a_goal_item() {
    let pending = Waiting {
        goal: None,
        source_id: "q_1".to_owned(),
        ..goal_item("met", AttentionState::NeedsInput)
    };
    let keys: Vec<String> = payload_of(&pending)
        .as_object()
        .expect("object")
        .keys()
        .cloned()
        .collect();
    assert_eq!(keys, ["run_id", "source_id", "state", "thread_id"]);
}

#[test]
fn a_goal_item_and_a_question_on_the_same_run_and_state_are_two_events() {
    let goal = goal_item("blocked", AttentionState::NeedsInput);
    let question = Waiting {
        goal: None,
        source_id: "q_1".to_owned(),
        ..goal.clone()
    };
    assert_ne!(
        run_attention_event_id(&goal),
        run_attention_event_id(&question)
    );
    assert_eq!(
        run_attention_event_id(&goal),
        run_attention_event_id(&goal.clone()),
        "a repeat sweep writes nothing"
    );
}

#[test]
fn the_goal_pass_reads_only_stopped_goals_of_a_listed_thread_with_a_settled_run() {
    let sql: String = GOALS_SQL.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(sql.contains("WHERE g.status IN ('met', 'budget_reached', 'turns_reached', 'blocked')"));
    assert!(sql.contains(
        "s.settled_at IS NOT NULL AND s.run_id IS NOT NULL ORDER BY s.turn_no DESC LIMIT 1"
    ));
    assert!(sql.contains("AND t.origin = ANY($2)"));
}
