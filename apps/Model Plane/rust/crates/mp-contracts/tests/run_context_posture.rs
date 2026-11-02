//! Posture table for the run context (build plan WS02 S4): which parts a run may
//! carry under each retention posture. One row per (posture, part); a new store
//! of conversation content adds a row here, like the ZDR retention matrix.

use mp_contracts::model_plane::v1::{ChatMessage, RunContext};
use mp_contracts::run_context::{validate, RunContextRefusal};

fn with_instructions() -> RunContext {
    RunContext {
        authored_instructions: "Answer in Norwegian.".to_owned(),
        ..Default::default()
    }
}

fn with_history() -> RunContext {
    RunContext {
        history: vec![ChatMessage {
            role: "user".to_owned(),
            content: "an earlier question".to_owned(),
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn with_memory() -> RunContext {
    RunContext {
        memory_block: "prefers short answers".to_owned(),
        ..Default::default()
    }
}

#[test]
fn the_posture_table() {
    // (name, context, zdr, expected)
    let rows: Vec<(&str, RunContext, bool, Result<(), RunContextRefusal>)> = vec![
        (
            "persistent: instructions",
            with_instructions(),
            false,
            Ok(()),
        ),
        ("persistent: history", with_history(), false, Ok(())),
        ("persistent: memory", with_memory(), false, Ok(())),
        ("zdr: instructions", with_instructions(), true, Ok(())),
        (
            "zdr: history",
            with_history(),
            true,
            Err(RunContextRefusal::ZdrHistory),
        ),
        (
            "zdr: memory",
            with_memory(),
            true,
            Err(RunContextRefusal::ZdrMemory),
        ),
    ];
    for (name, context, zdr, expected) in rows {
        assert_eq!(validate(Some(&context), zdr), expected, "row: {name}");
    }
    // Absent = today's behaviour under every posture.
    assert_eq!(validate(None, false), Ok(()));
    assert_eq!(validate(None, true), Ok(()));
}
