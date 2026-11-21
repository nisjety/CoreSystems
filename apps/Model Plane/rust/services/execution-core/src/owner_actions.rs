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
    /// The model-access class a view may name for this pin. A closed set of one: a view that names
    /// any other class is refused, and a view that names none is not checked on this field.
    pub class: &'static str,
    /// The authority a view may name for this pin, closed the same way as `class`.
    pub authority: &'static str,
}

pub const OWNER_ACTION_PINS: &[OwnerActionPin] = &[OwnerActionPin {
    name: crate::ticket_tools::TOOL_NAME,
    schema_sha256: crate::ticket_tools::TICKET_CREATE_SCHEMA_SHA256,
    parameters_json: crate::ticket_tools::TICKET_CREATE_MODEL_PARAMETERS_JSON,
    adapter: OwnerActionAdapter::TicketCreate,
    requires_durable_approval: true,
    class: "write_approval",
    authority: "owner_contract",
}];

/// A pinned owner action, found by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    #[must_use]
    pub fn name(self) -> &'static str {
        self.0.name
    }

    #[must_use]
    pub fn class(self) -> &'static str {
        self.0.class
    }

    #[must_use]
    pub fn authority(self) -> &'static str {
        self.0.authority
    }
}

/// The pin `tool` matches out of `pins`: its exact pinned name, a non-empty description, the pinned
/// parameters and the pinned schema hash. A definition that fails any one of them matches no pin.
#[must_use]
pub fn match_pin<'a>(
    pins: &'a [OwnerActionPin],
    tool: &ToolDefinition,
    schema_sha256: &str,
) -> Option<&'a OwnerActionPin> {
    pins.iter().find(|pin| pin.name == tool.name).filter(|pin| {
        !tool.description.trim().is_empty()
            && tool.parameters_json == pin.parameters_json
            && schema_sha256 == pin.schema_sha256
    })
}

/// The shipped pin `tool` matches, by name, description, parameters and schema hash.
#[must_use]
pub fn definition_matches_pin(
    tool: &ToolDefinition,
    schema_sha256: &str,
) -> Option<ServerResolvedAction> {
    match_pin(OWNER_ACTION_PINS, tool, schema_sha256).map(ServerResolvedAction)
}

/// The pins a run view may name in this build: every pin while the admission table is on. While it
/// is off, only the tickets.create pin that shipped before the table existed.
#[must_use]
pub fn offered_pins(admission_table: bool) -> Vec<OwnerActionPin> {
    OWNER_ACTION_PINS
        .iter()
        .copied()
        .filter(|pin| admission_table || pin.name == crate::ticket_tools::TOOL_NAME)
        .collect()
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

    const PARAMS_ALPHA: &str = r#"{"type":"object","properties":{"a":{"type":"string"}}}"#;
    const PARAMS_BETA: &str = r#"{"type":"object","properties":{"b":{"type":"string"}}}"#;
    const HASH_ALPHA: &str =
        "sha256:1111111111111111111111111111111111111111111111111111111111111111";
    const HASH_BETA: &str =
        "sha256:2222222222222222222222222222222222222222222222222222222222222222";

    // Two synthetic pins, so the N-pin matching is exercised without shipping a second action.
    const TWO_PINS: [OwnerActionPin; 2] = [
        OwnerActionPin {
            name: "alpha.one",
            schema_sha256: HASH_ALPHA,
            parameters_json: PARAMS_ALPHA,
            adapter: OwnerActionAdapter::TicketCreate,
            requires_durable_approval: false,
            class: "read",
            authority: "owner_contract",
        },
        OwnerActionPin {
            name: "beta.two",
            schema_sha256: HASH_BETA,
            parameters_json: PARAMS_BETA,
            adapter: OwnerActionAdapter::TicketCreate,
            requires_durable_approval: true,
            class: "write_approval",
            authority: "owner_contract",
        },
    ];

    fn described(name: &str, description: &str, parameters_json: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_owned(),
            description: description.to_owned(),
            parameters_json: parameters_json.to_owned(),
        }
    }

    #[test]
    fn each_definition_matches_only_its_own_pin_by_name_hash_and_parameters() {
        let alpha = described("alpha.one", "reads alpha", PARAMS_ALPHA);
        assert_eq!(
            match_pin(&TWO_PINS, &alpha, HASH_ALPHA).map(|pin| pin.name),
            Some("alpha.one")
        );
        // A read pin is matched too: the approval flag is checked by the caller, not by the match.
        assert!(!TWO_PINS[0].requires_durable_approval);
        // The other pin's hash, the other pin's parameters, an unpinned name and a blank
        // description each match nothing.
        assert!(match_pin(&TWO_PINS, &alpha, HASH_BETA).is_none());
        assert!(match_pin(
            &TWO_PINS,
            &described("alpha.one", "reads alpha", PARAMS_BETA),
            HASH_ALPHA
        )
        .is_none());
        assert!(match_pin(
            &TWO_PINS,
            &described("gamma.three", "unpinned", PARAMS_ALPHA),
            HASH_ALPHA
        )
        .is_none());
        assert!(match_pin(
            &TWO_PINS,
            &described("alpha.one", "   ", PARAMS_ALPHA),
            HASH_ALPHA
        )
        .is_none());
        let beta = described("beta.two", "writes beta", PARAMS_BETA);
        assert_eq!(
            match_pin(&TWO_PINS, &beta, HASH_BETA).map(|pin| pin.name),
            Some("beta.two")
        );
    }

    #[test]
    fn the_shipped_ticket_pin_is_matched_by_hash_and_closed_to_its_class_and_authority() {
        let ticket = resolved(
            crate::ticket_tools::TOOL_NAME,
            crate::ticket_tools::TICKET_CREATE_MODEL_PARAMETERS_JSON,
        );
        let action =
            definition_matches_pin(&ticket, crate::ticket_tools::TICKET_CREATE_SCHEMA_SHA256)
                .expect("the shipped ticket definition matches its pin");
        assert!(action.requires_durable_approval());
        assert_eq!(action.name(), crate::ticket_tools::TOOL_NAME);
        assert_eq!(action.class(), "write_approval");
        assert_eq!(action.authority(), "owner_contract");
        assert!(definition_matches_pin(&ticket, &format!("sha256:{}", "0".repeat(64))).is_none());
    }

    #[test]
    fn the_offered_pins_are_every_pin_with_the_table_and_only_tickets_without_it() {
        let names = |pins: Vec<OwnerActionPin>| pins.iter().map(|pin| pin.name).collect::<Vec<_>>();
        assert_eq!(
            names(offered_pins(false)),
            vec![crate::ticket_tools::TOOL_NAME]
        );
        assert_eq!(
            names(offered_pins(true)),
            OWNER_ACTION_PINS
                .iter()
                .map(|pin| pin.name)
                .collect::<Vec<_>>()
        );
    }
}
