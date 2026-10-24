//! Owner actions the Model may be offered, pinned in code.
//!
//! Capability Core's `agent_action_admissions` table says which owner actions are
//! enabled, but a table row alone admits nothing. Execution Core offers and executes
//! an owner action only when this build pins its name, schema hash, parameters and
//! adapter, so a compromised catalog or table row cannot add a tool or reshape one
//! this build does not execute. Each call still passes the owner's own verification.

use mp_contracts::model_plane::v1::ToolDefinition;

/// The executor that performs a pinned owner action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerActionAdapter {
    /// Conversation Core tickets, through the Control decision and owner receipt in `ticket_tools`.
    TicketCreate,
}

/// One owner action this build offers and executes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerActionPin {
    pub name: &'static str,
    pub schema_sha256: &'static str,
    pub parameters_json: &'static str,
    pub adapter: OwnerActionAdapter,
    pub requires_durable_approval: bool,
}

pub const OWNER_ACTION_PINS: &[OwnerActionPin] = &[OwnerActionPin {
    name: crate::ticket_tools::TOOL_NAME,
    schema_sha256: crate::ticket_tools::TICKET_CREATE_SCHEMA_SHA256,
    parameters_json: crate::ticket_tools::TICKET_CREATE_MODEL_PARAMETERS_JSON,
    adapter: OwnerActionAdapter::TicketCreate,
    requires_durable_approval: true,
}];

/// A pinned owner action, found by name.
#[derive(Debug, Clone, Copy)]
pub struct ServerResolvedAction(&'static OwnerActionPin);

impl ServerResolvedAction {
    /// The pin for `name`. The trim matches every ingress, so whitespace variants resolve to the
    /// same pin, while [`Self::admits`] still requires the exact name.
    #[must_use]
    pub fn pinned(name: &str) -> Option<Self> {
        let name = name.trim();
        OWNER_ACTION_PINS
            .iter()
            .find(|pin| pin.name == name)
            .map(Self)
    }

    /// Whether a resolved definition may join the offered set: the exact pinned name, a
    /// non-empty description and the exact pinned parameters. Capability Core checks the schema
    /// hash against the same constant when it decodes the view.
    #[must_use]
    pub fn admits(self, tool: &ToolDefinition) -> bool {
        tool.name == self.0.name
            && !tool.description.trim().is_empty()
            && tool.parameters_json == self.0.parameters_json
    }

    #[must_use]
    pub fn requires_durable_approval(self) -> bool {
        self.0.requires_durable_approval
    }
}

/// Whether `name` is a reserved owner action, which only the run-bound resolver may offer.
#[must_use]
pub fn is_reserved(name: &str) -> bool {
    ServerResolvedAction::pinned(name).is_some()
}

/// Whether a durable, bound owner approval must precede this tool's adapter.
#[must_use]
pub fn requires_durable_approval(tool_name: &str) -> bool {
    ServerResolvedAction::pinned(tool_name)
        .is_some_and(ServerResolvedAction::requires_durable_approval)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolved(name: &str, parameters_json: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_owned(),
            description: "Control-resolved owner action".to_owned(),
            parameters_json: parameters_json.to_owned(),
        }
    }

    #[test]
    fn an_unpinned_server_action_is_dropped() {
        let alias = resolved(
            "tickets.create.alias",
            crate::ticket_tools::TICKET_CREATE_MODEL_PARAMETERS_JSON,
        );
        assert!(ServerResolvedAction::pinned(&alias.name).is_none());
        assert!(!is_reserved(&alias.name));

        // A whitespace variant resolves to the pin but is not the exact pinned name, so it is
        // still refused as an offered tool.
        let padded = resolved(
            " tickets.create",
            crate::ticket_tools::TICKET_CREATE_MODEL_PARAMETERS_JSON,
        );
        let action = ServerResolvedAction::pinned(&padded.name).expect("trim resolves the pin");
        assert!(!action.admits(&padded));
    }

    #[test]
    fn a_pinned_action_with_a_drifted_schema_is_dropped() {
        let action = ServerResolvedAction::pinned(crate::ticket_tools::TOOL_NAME)
            .expect("tickets.create is pinned");
        let drifted = resolved(
            crate::ticket_tools::TOOL_NAME,
            r#"{"type":"object","additionalProperties":true}"#,
        );
        assert!(!action.admits(&drifted));

        let pinned = resolved(
            crate::ticket_tools::TOOL_NAME,
            crate::ticket_tools::TICKET_CREATE_MODEL_PARAMETERS_JSON,
        );
        assert!(action.admits(&pinned));
    }

    #[test]
    fn only_the_pinned_ticket_action_requires_durable_approval() {
        assert!(requires_durable_approval(crate::ticket_tools::TOOL_NAME));
        assert!(!requires_durable_approval("web_fetch"));
        assert!(!requires_durable_approval("knowledge.list_documents"));
    }
}
