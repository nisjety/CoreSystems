//! Subagent lifecycle helpers.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentLifecycle {
    pub status: String,
    pub summary: String,
}

#[must_use]
pub fn maybe_spawn(tool_name: &str) -> Option<SubagentLifecycle> {
    if !tool_name.starts_with("subagent.") {
        return None;
    }

    Some(SubagentLifecycle {
        status: "started".to_owned(),
        summary: format!("spawned {tool_name}"),
    })
}
