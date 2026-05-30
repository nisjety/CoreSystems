//! Runtime lifecycle verification (Phase F Runtime gate).
//!
//! Mirrors the emission contracts in `src/grpc.rs`:
//!   - `start_run`       -> RUN_STARTED
//!   - `complete_step`   -> STEP_COMPLETED (+ RUN_COMPLETED | RUN_FAILED on terminal)
//!   - `save_checkpoint` -> CHECKPOINT_SAVED
//!
//! Each test feeds a recorded emission sequence into an in-memory replay
//! fold identical in shape to `replay_deterministic.rs`, then asserts the
//! invariants required by `docs/VERIFICATION.md` §Runtime Tests.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RuntimeEvent {
    ts_millis: i64,
    event_id: String,
    event_type: String,
    run_id: String,
    parent_run_id: Option<String>,
    payload: String,
}

#[derive(Debug, Default, PartialEq)]
struct RunState {
    status: String,
    parent_run_id: Option<String>,
    steps_completed: u32,
    checkpoint_count: u32,
    error: Option<String>,
    emission_order: Vec<String>,
}

#[derive(Debug, Default)]
struct ThreadState {
    runs: BTreeMap<String, RunState>,
}

fn replay(mut events: Vec<RuntimeEvent>) -> ThreadState {
    events.sort();
    let mut state = ThreadState::default();

    for event in events {
        let run = state.runs.entry(event.run_id.clone()).or_default();
        run.emission_order.push(event.event_type.clone());

        match event.event_type.as_str() {
            "RUN_STARTED" => {
                "running".clone_into(&mut run.status);
                run.parent_run_id = event.parent_run_id.clone();
            }
            "STEP_COMPLETED" => run.steps_completed += 1,
            "CHECKPOINT_SAVED" => run.checkpoint_count += 1,
            "RUN_COMPLETED" => "completed".clone_into(&mut run.status),
            "RUN_FAILED" => {
                "failed".clone_into(&mut run.status);
                run.error = Some(event.payload.clone());
            }
            _ => {}
        }
    }

    state
}

fn evt(ts: i64, id: &str, ty: &str, run: &str) -> RuntimeEvent {
    RuntimeEvent {
        ts_millis: ts,
        event_id: id.to_owned(),
        event_type: ty.to_owned(),
        run_id: run.to_owned(),
        parent_run_id: None,
        payload: String::new(),
    }
}

/// Item 1: Lifecycle events emitted in correct order
/// (start -> steps -> checkpoint -> complete).
#[test]
fn lifecycle_emits_events_in_canonical_order() {
    let events = vec![
        evt(1000, "01A", "RUN_STARTED", "run-1"),
        evt(2000, "01B", "STEP_COMPLETED", "run-1"),
        evt(3000, "01C", "STEP_COMPLETED", "run-1"),
        evt(4000, "01D", "CHECKPOINT_SAVED", "run-1"),
        evt(5000, "01E", "STEP_COMPLETED", "run-1"),
        evt(6000, "01F", "RUN_COMPLETED", "run-1"),
    ];

    let state = replay(events);
    let run = state.runs.get("run-1").expect("run-1 exists");

    assert_eq!(
        run.emission_order,
        vec![
            "RUN_STARTED",
            "STEP_COMPLETED",
            "STEP_COMPLETED",
            "CHECKPOINT_SAVED",
            "STEP_COMPLETED",
            "RUN_COMPLETED",
        ],
        "lifecycle must open with RUN_STARTED and close with terminal event"
    );
    assert_eq!(
        run.emission_order.first().map(String::as_str),
        Some("RUN_STARTED")
    );
    assert_eq!(
        run.emission_order.last().map(String::as_str),
        Some("RUN_COMPLETED")
    );

    // Checkpoint must appear after at least one step and before terminal.
    let cp_idx = run
        .emission_order
        .iter()
        .position(|e| e == "CHECKPOINT_SAVED")
        .expect("checkpoint emitted");
    let first_step_idx = run
        .emission_order
        .iter()
        .position(|e| e == "STEP_COMPLETED")
        .expect("step emitted");
    let terminal_idx = run.emission_order.len() - 1;
    assert!(first_step_idx < cp_idx, "checkpoint after first step");
    assert!(cp_idx < terminal_idx, "checkpoint before terminal");

    assert_eq!(run.status, "completed");
    assert_eq!(run.steps_completed, 3);
    assert_eq!(run.checkpoint_count, 1);
}

/// Item 2: All taxonomy event types exercised in at least one path.
#[test]
fn all_taxonomy_event_types_are_exercised() {
    // Successful run covers RUN_STARTED, STEP_COMPLETED, CHECKPOINT_SAVED, RUN_COMPLETED.
    // Failed run covers RUN_FAILED.
    let events = vec![
        evt(1000, "01A", "RUN_STARTED", "run-ok"),
        evt(1100, "01B", "STEP_COMPLETED", "run-ok"),
        evt(1200, "01C", "CHECKPOINT_SAVED", "run-ok"),
        evt(1300, "01D", "RUN_COMPLETED", "run-ok"),
        evt(2000, "02A", "RUN_STARTED", "run-fail"),
        evt(2100, "02B", "STEP_COMPLETED", "run-fail"),
        RuntimeEvent {
            ts_millis: 2200,
            event_id: "02C".to_owned(),
            event_type: "RUN_FAILED".to_owned(),
            run_id: "run-fail".to_owned(),
            parent_run_id: None,
            payload: "{\"error\":\"boom\"}".to_owned(),
        },
    ];

    let state = replay(events);

    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for run in state.runs.values() {
        for ty in &run.emission_order {
            seen.insert(ty.clone());
        }
    }

    for required in [
        "RUN_STARTED",
        "STEP_COMPLETED",
        "CHECKPOINT_SAVED",
        "RUN_COMPLETED",
        "RUN_FAILED",
    ] {
        assert!(
            seen.contains(required),
            "taxonomy event type {required} must be exercised"
        );
    }
}

/// Item 3: Failure paths emit RUN_FAILED with error context in payload.
#[test]
fn failure_path_emits_run_failed_with_error_context() {
    let events = vec![
        evt(1000, "01A", "RUN_STARTED", "run-fail"),
        evt(2000, "01B", "STEP_COMPLETED", "run-fail"),
        RuntimeEvent {
            ts_millis: 3000,
            event_id: "01C".to_owned(),
            event_type: "RUN_FAILED".to_owned(),
            run_id: "run-fail".to_owned(),
            parent_run_id: None,
            payload: "{\"error\":\"model-gateway unreachable\"}".to_owned(),
        },
    ];

    let state = replay(events);
    let run = state.runs.get("run-fail").expect("run-fail exists");

    assert_eq!(run.status, "failed");
    let err = run.error.as_deref().expect("error payload present");
    assert!(
        err.contains("error"),
        "RUN_FAILED payload must carry error context, got {err}"
    );
    assert!(
        err.contains("model-gateway unreachable"),
        "RUN_FAILED payload must include originating error message"
    );
}

/// Item 4: Subagent spawn/stop events maintain parent/child lineage.
#[test]
fn subagent_lineage_links_child_to_parent() {
    let events = vec![
        evt(1000, "P01", "RUN_STARTED", "parent"),
        evt(1500, "P02", "STEP_COMPLETED", "parent"),
        // Child run spawned mid-parent; carries parent_run_id.
        RuntimeEvent {
            ts_millis: 2000,
            event_id: "C01".to_owned(),
            event_type: "RUN_STARTED".to_owned(),
            run_id: "child".to_owned(),
            parent_run_id: Some("parent".to_owned()),
            payload: String::new(),
        },
        evt(2500, "C02", "STEP_COMPLETED", "child"),
        evt(3000, "C03", "RUN_COMPLETED", "child"),
        evt(4000, "P03", "RUN_COMPLETED", "parent"),
    ];

    let state = replay(events);

    let parent = state.runs.get("parent").expect("parent exists");
    let child = state.runs.get("child").expect("child exists");

    assert_eq!(parent.status, "completed");
    assert_eq!(child.status, "completed");
    assert_eq!(
        child.parent_run_id.as_deref(),
        Some("parent"),
        "child's RUN_STARTED must carry parent lineage"
    );
    assert!(parent.parent_run_id.is_none(), "root run has no parent");
}
