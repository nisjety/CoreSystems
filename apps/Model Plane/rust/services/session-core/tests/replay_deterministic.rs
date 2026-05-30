//! Deterministic replay test skeleton.
//!
//! Feeds a sequence of events into an in-memory store and asserts:
//! 1. Final thread/run/checkpoint state equals a golden snapshot.
//! 2. Ordering within same timestamp breaks deterministically via `event_id` tiebreak.
//!
//! This test does NOT require a running Postgres instance — it operates
//! on in-memory data structures that mirror the replay logic.

use std::collections::BTreeMap;

/// Minimal in-memory event for replay testing.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ReplayEvent {
    ts_millis: i64,
    event_id: String,
    event_type: String,
    run_id: String,
    payload: String,
}

/// Minimal thread state rebuilt from replay.
#[derive(Debug, Default, PartialEq)]
struct ThreadState {
    runs: BTreeMap<String, RunState>,
    event_count: usize,
}

#[derive(Debug, Default, PartialEq)]
struct RunState {
    status: String,
    steps_completed: u32,
    checkpoint_count: u32,
    error: Option<String>,
}

/// Replay events in deterministic order: sort by (ts, `event_id`).
fn replay(
    events: &mut Vec<ReplayEvent>,
    after_event_id: Option<&str>,
    limit: usize,
) -> ThreadState {
    events.sort();

    if let Some(cursor) = after_event_id {
        events.retain(|e| e.event_id.as_str() > cursor);
    }
    if limit > 0 && events.len() > limit {
        events.truncate(limit);
    }

    let mut state = ThreadState::default();

    for event in events {
        state.event_count += 1;
        let run = state.runs.entry(event.run_id.clone()).or_default();

        match event.event_type.as_str() {
            "RUN_STARTED" => "running".clone_into(&mut run.status),
            "RUN_COMPLETED" => "completed".clone_into(&mut run.status),
            "RUN_FAILED" => {
                "failed".clone_into(&mut run.status);
                run.error = Some(event.payload.clone());
            }
            "STEP_COMPLETED" => run.steps_completed += 1,
            "CHECKPOINT_SAVED" => run.checkpoint_count += 1,
            _ => {}
        }
    }

    state
}

#[test]
fn replay_produces_correct_final_state() {
    let mut events = vec![
        ReplayEvent {
            ts_millis: 1000,
            event_id: "01A".to_owned(),
            event_type: "RUN_STARTED".to_owned(),
            run_id: "run-1".to_owned(),
            payload: String::new(),
        },
        ReplayEvent {
            ts_millis: 2000,
            event_id: "01B".to_owned(),
            event_type: "STEP_COMPLETED".to_owned(),
            run_id: "run-1".to_owned(),
            payload: String::new(),
        },
        ReplayEvent {
            ts_millis: 2000,
            event_id: "01C".to_owned(),
            event_type: "STEP_COMPLETED".to_owned(),
            run_id: "run-1".to_owned(),
            payload: String::new(),
        },
        ReplayEvent {
            ts_millis: 3000,
            event_id: "01D".to_owned(),
            event_type: "CHECKPOINT_SAVED".to_owned(),
            run_id: "run-1".to_owned(),
            payload: String::new(),
        },
        ReplayEvent {
            ts_millis: 4000,
            event_id: "01E".to_owned(),
            event_type: "RUN_COMPLETED".to_owned(),
            run_id: "run-1".to_owned(),
            payload: String::new(),
        },
    ];

    let state = replay(&mut events, None, 0);

    assert_eq!(state.event_count, 5);
    let run = state.runs.get("run-1").expect("run-1 should exist");
    assert_eq!(run.status, "completed");
    assert_eq!(run.steps_completed, 2);
    assert_eq!(run.checkpoint_count, 1);
}

#[test]
fn replay_is_deterministic_regardless_of_input_order() {
    let base = vec![
        ReplayEvent {
            ts_millis: 1000,
            event_id: "01A".to_owned(),
            event_type: "RUN_STARTED".to_owned(),
            run_id: "run-1".to_owned(),
            payload: String::new(),
        },
        ReplayEvent {
            ts_millis: 2000,
            event_id: "01C".to_owned(),
            event_type: "STEP_COMPLETED".to_owned(),
            run_id: "run-1".to_owned(),
            payload: String::new(),
        },
        ReplayEvent {
            ts_millis: 2000,
            event_id: "01B".to_owned(),
            event_type: "STEP_COMPLETED".to_owned(),
            run_id: "run-1".to_owned(),
            payload: String::new(),
        },
    ];

    // Forward order
    let mut forward = base.clone();
    let state1 = replay(&mut forward, None, 0);

    // Reverse order
    let mut reverse: Vec<_> = base.into_iter().rev().collect();
    let state2 = replay(&mut reverse, None, 0);

    assert_eq!(
        state1, state2,
        "replay must be deterministic regardless of input order"
    );
}

#[test]
fn replay_records_run_failed_with_error_payload() {
    let mut events = vec![
        ReplayEvent {
            ts_millis: 1000,
            event_id: "02A".to_owned(),
            event_type: "RUN_STARTED".to_owned(),
            run_id: "run-x".to_owned(),
            payload: String::new(),
        },
        ReplayEvent {
            ts_millis: 2000,
            event_id: "02B".to_owned(),
            event_type: "RUN_FAILED".to_owned(),
            run_id: "run-x".to_owned(),
            payload: "boom".to_owned(),
        },
    ];

    let state = replay(&mut events, None, 0);
    let run = state.runs.get("run-x").expect("run-x should exist");
    assert_eq!(run.status, "failed");
    assert_eq!(run.error.as_deref(), Some("boom"));
}

#[test]
fn replay_cursor_filters_events_by_id() {
    let mut events = vec![
        ReplayEvent {
            ts_millis: 1000,
            event_id: "03A".to_owned(),
            event_type: "RUN_STARTED".to_owned(),
            run_id: "run-c".to_owned(),
            payload: String::new(),
        },
        ReplayEvent {
            ts_millis: 2000,
            event_id: "03B".to_owned(),
            event_type: "STEP_COMPLETED".to_owned(),
            run_id: "run-c".to_owned(),
            payload: String::new(),
        },
        ReplayEvent {
            ts_millis: 3000,
            event_id: "03C".to_owned(),
            event_type: "STEP_COMPLETED".to_owned(),
            run_id: "run-c".to_owned(),
            payload: String::new(),
        },
    ];

    let state = replay(&mut events, Some("03A"), 0);
    assert_eq!(state.event_count, 2);
    let run = state.runs.get("run-c").expect("run-c should exist");
    assert_eq!(run.status, "");
    assert_eq!(run.steps_completed, 2);
}

#[test]
fn replay_limit_truncates_events() {
    let mut events = vec![
        ReplayEvent {
            ts_millis: 1000,
            event_id: "04A".to_owned(),
            event_type: "RUN_STARTED".to_owned(),
            run_id: "run-l".to_owned(),
            payload: String::new(),
        },
        ReplayEvent {
            ts_millis: 2000,
            event_id: "04B".to_owned(),
            event_type: "STEP_COMPLETED".to_owned(),
            run_id: "run-l".to_owned(),
            payload: String::new(),
        },
        ReplayEvent {
            ts_millis: 3000,
            event_id: "04C".to_owned(),
            event_type: "RUN_COMPLETED".to_owned(),
            run_id: "run-l".to_owned(),
            payload: String::new(),
        },
    ];

    let state = replay(&mut events, None, 2);
    assert_eq!(state.event_count, 2);
    let run = state.runs.get("run-l").expect("run-l should exist");
    assert_eq!(run.status, "running");
    assert_eq!(run.steps_completed, 1);
}

#[test]
fn replay_empty_events_returns_default() {
    let mut events: Vec<ReplayEvent> = vec![];
    let state = replay(&mut events, None, 0);
    assert_eq!(state, ThreadState::default());
}

#[test]
fn replay_isolates_multiple_runs() {
    let mut events = vec![
        ReplayEvent {
            ts_millis: 1000,
            event_id: "05A".to_owned(),
            event_type: "RUN_STARTED".to_owned(),
            run_id: "run-a".to_owned(),
            payload: String::new(),
        },
        ReplayEvent {
            ts_millis: 1500,
            event_id: "05B".to_owned(),
            event_type: "RUN_STARTED".to_owned(),
            run_id: "run-b".to_owned(),
            payload: String::new(),
        },
        ReplayEvent {
            ts_millis: 2000,
            event_id: "05C".to_owned(),
            event_type: "STEP_COMPLETED".to_owned(),
            run_id: "run-a".to_owned(),
            payload: String::new(),
        },
        ReplayEvent {
            ts_millis: 2500,
            event_id: "05D".to_owned(),
            event_type: "RUN_COMPLETED".to_owned(),
            run_id: "run-b".to_owned(),
            payload: String::new(),
        },
    ];

    let state = replay(&mut events, None, 0);
    assert_eq!(state.event_count, 4);
    assert_eq!(state.runs.len(), 2);
    assert_eq!(state.runs.get("run-a").unwrap().status, "running");
    assert_eq!(state.runs.get("run-a").unwrap().steps_completed, 1);
    assert_eq!(state.runs.get("run-b").unwrap().status, "completed");
}

/// Deduplicates events by idempotency_key (keeps first occurrence).
/// Empty keys are passthrough (no dedup).
/// Mirrors the `mp_events_idempotency_key` uniqueness constraint in Postgres.
fn dedup_by_idempotency(events: &[ReplayEvent], keys: &[&str]) -> Vec<ReplayEvent> {
    use std::collections::HashSet;
    assert_eq!(events.len(), keys.len(), "events and keys must align");
    let mut seen: HashSet<String> = HashSet::new();
    events
        .iter()
        .zip(keys.iter())
        .filter(|(_, k)| k.is_empty() || seen.insert(k.to_string()))
        .map(|(e, _)| e.clone())
        .collect()
}

#[test]
fn dedup_drops_events_with_duplicate_idempotency_key() {
    let events = vec![
        ReplayEvent {
            ts_millis: 1000,
            event_id: "06A".to_owned(),
            event_type: "RUN_STARTED".to_owned(),
            run_id: "run-d".to_owned(),
            payload: String::new(),
        },
        ReplayEvent {
            ts_millis: 2000,
            event_id: "06B".to_owned(),
            event_type: "STEP_COMPLETED".to_owned(),
            run_id: "run-d".to_owned(),
            payload: String::new(),
        },
        ReplayEvent {
            ts_millis: 2001,
            event_id: "06C".to_owned(),
            event_type: "STEP_COMPLETED".to_owned(),
            run_id: "run-d".to_owned(),
            payload: String::new(),
        },
    ];
    // 06B and 06C share an idempotency key → 06C must be dropped.
    let keys = ["k-start", "k-step-1", "k-step-1"];

    let mut deduped = dedup_by_idempotency(&events, &keys);
    assert_eq!(
        deduped.len(),
        2,
        "duplicate idempotency key must be dropped"
    );

    let state = replay(&mut deduped, None, 0);
    assert_eq!(state.event_count, 2);
    let run = state.runs.get("run-d").expect("run-d should exist");
    assert_eq!(run.status, "running");
    assert_eq!(
        run.steps_completed, 1,
        "only first of duplicate-keyed events counts"
    );
}

#[test]
fn dedup_empty_keys_are_passthrough() {
    let events = vec![
        ReplayEvent {
            ts_millis: 1000,
            event_id: "07A".to_owned(),
            event_type: "RUN_STARTED".to_owned(),
            run_id: "run-e".to_owned(),
            payload: String::new(),
        },
        ReplayEvent {
            ts_millis: 2000,
            event_id: "07B".to_owned(),
            event_type: "STEP_COMPLETED".to_owned(),
            run_id: "run-e".to_owned(),
            payload: String::new(),
        },
    ];
    let keys = ["", ""];
    let deduped = dedup_by_idempotency(&events, &keys);
    assert_eq!(deduped.len(), 2, "empty idempotency keys skip dedup");
}
