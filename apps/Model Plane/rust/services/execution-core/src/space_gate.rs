//! What an approval card says about a gated call (build plan WS10-S1).
//!
//! The card context is built here from an allowlist: a stable `tool_key`, a short `action_label`
//! and the `resume_class` the pause expects. Raw tool arguments never enter it. Session Core
//! validates the object again and decides the resume class from the rows it holds (a continuation
//! descriptor makes it an action continuation), so this module states intent, not authority.
//!
//! The three resumable tool names live here once; the approval delivery worker reads them from
//! this module so the list the card promises and the list the worker can run cannot drift
//! (WS10-S9 adds the parity test and the proof checklist).

use serde_json::json;

/// Provider write run by the approval delivery worker from its descriptor.
pub(crate) const PROVIDER_ACTION_TOOL: &str = "execute_provider_action";
/// Shipment booking run by the approval delivery worker from its descriptor.
pub(crate) const SHIPMENT_BOOKING_TOOL: &str = "book_shipment";
/// Ticket creation run by the approval delivery worker from its descriptor.
pub(crate) const TICKETS_CREATE_TOOL: &str = "tickets.create";
/// The tools whose granted approval the worker can run (`action_continuation`). Read by the
/// WS10-S9 parity test; the worker still matches the three names directly.
#[allow(dead_code)]
pub(crate) const RESUMABLE_TOOLS: &[&str] = &[
    PROVIDER_ACTION_TOOL,
    SHIPMENT_BOOKING_TOOL,
    TICKETS_CREATE_TOOL,
];

/// Resume class names, the same words Session Core stores.
pub(crate) const RESUME_ACTION_CONTINUATION: &str = "action_continuation";
pub(crate) const RESUME_LIVE_POLL: &str = "live_poll";
pub(crate) const RESUME_NONE: &str = "none";

/// Longest value Session Core accepts in the card context.
const MAX_VALUE_CHARS: usize = 128;

/// A name that is safe to show: ASCII letters, digits and `_ . : - /`, at most 64 characters.
fn safe_identifier(value: &str) -> Option<&str> {
    let ok = !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-' | b'/'));
    ok.then_some(value)
}

/// The `tool_key` of a call: the tool's own name when it is a plain identifier, else empty.
pub(crate) fn tool_key(tool_name: &str) -> &str {
    safe_identifier(tool_name).unwrap_or_default()
}

/// A short label for the gated action, from an allowlist. Arguments are read only for the
/// provider operation name, and only when it is a plain identifier.
pub(crate) fn action_label(tool_name: &str, tool_input: &str) -> String {
    let label = match tool_name {
        PROVIDER_ACTION_TOOL => {
            let operation = serde_json::from_str::<serde_json::Value>(tool_input)
                .ok()
                .and_then(|input| input.get("operation")?.as_str().map(str::to_owned));
            match operation.as_deref().and_then(safe_identifier) {
                Some(operation) => format!("Run provider action {operation}"),
                None => "Run a provider action".to_owned(),
            }
        }
        SHIPMENT_BOOKING_TOOL => "Book a shipment".to_owned(),
        TICKETS_CREATE_TOOL => "Create a ticket".to_owned(),
        "shell" => "Run a command in the sandbox".to_owned(),
        "browser_agent" => "Run a browser action".to_owned(),
        other => match safe_identifier(other) {
            Some(name) => format!("Use the tool {name}"),
            None => "Use a tool".to_owned(),
        },
    };
    label.chars().take(MAX_VALUE_CHARS).collect()
}

/// The `CreateApprovalRequest.context_json` of a paused tool call. `resume_class` states what the
/// pause expects; Session Core overrides it from its own rows.
pub(crate) fn card_context_json(tool_name: &str, tool_input: &str, resume_class: &str) -> String {
    json!({
        "tool_key": tool_key(tool_name),
        "action_label": action_label(tool_name, tool_input),
        "resume_class": resume_class,
    })
    .to_string()
}
