//! Hook context parsing and pre-execution checks.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct HookContext {
    #[serde(default)]
    block_execution: bool,
}

#[must_use]
pub fn is_blocked(raw_context: &str) -> bool {
    if raw_context.is_empty() {
        return false;
    }

    serde_json::from_str::<HookContext>(raw_context)
        .map(|ctx| ctx.block_execution)
        .unwrap_or(false)
}
