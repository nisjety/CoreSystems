//! Golden cases of the agent mode vocabulary (build plan WS25-S6).
//!
//! `api/contracts/agent-modes.json` is the vocabulary and its presets written out as data.
//! The composer, the gateway and this crate read the same ids, so a mode added or re-tuned
//! on one side and not the other fails here, and the pinned digest fails when the file
//! changes without its `.sha256` being refreshed.

use mp_contracts::agent_mode::{AgentMode, LOCKDOWN_CODE, UNKNOWN_MODE_CODE};
use mp_contracts::autonomy;
use serde_json::Value;
use sha2::{Digest, Sha256};

const GOLDEN: &str = include_str!("../../../../api/contracts/agent-modes.json");
const PIN: &str = include_str!("../../../../api/contracts/agent-modes.json.sha256");

fn golden() -> Value {
    serde_json::from_str(GOLDEN).expect("the golden is JSON")
}

#[test]
fn agent_mode_golden_lists_every_mode_strictest_first_with_its_preset() {
    let rows = golden()["modes"]
        .as_array()
        .cloned()
        .expect("modes is a list");
    assert_eq!(
        rows.len(),
        AgentMode::ALL.len(),
        "a mode was added to one side only"
    );
    for (row, mode) in rows.iter().zip(AgentMode::ALL) {
        let preset = mode.preset();
        assert_eq!(row["id"], mode.as_str(), "id of {mode:?}");
        assert_eq!(
            row["strictness"],
            u64::from(mode.strictness()),
            "strictness of {mode:?}"
        );
        assert_eq!(
            row["permission_mode"], preset.permission_mode,
            "permission of {mode:?}"
        );
        assert_eq!(row["plan_gate"], preset.plan_gate, "plan gate of {mode:?}");
        assert_eq!(
            row["rung"],
            autonomy::label(preset.rung),
            "rung of {mode:?}"
        );
        assert_eq!(row["do_runs"], preset.do_runs, "do_runs of {mode:?}");
    }
}

#[test]
fn agent_mode_golden_refusal_codes_are_the_ones_the_gateway_returns() {
    let document = golden();
    let refusals: Vec<&str> = document["refusals"]
        .as_array()
        .expect("refusals is a list")
        .iter()
        .map(|code| code.as_str().expect("a code is text"))
        .collect();
    assert_eq!(refusals, [UNKNOWN_MODE_CODE, LOCKDOWN_CODE]);
}

#[test]
fn agent_mode_golden_digest_is_the_pinned_one() {
    let normalised = GOLDEN.replace("\r\n", "\n");
    let digest: String = Sha256::digest(normalised.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let pinned = PIN
        .split_whitespace()
        .next()
        .expect("the pin names a digest");
    assert_eq!(
        digest, pinned,
        "agent-modes.json changed without its .sha256 pin being refreshed"
    );
}
