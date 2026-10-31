//! Declared side-effect classes for tools (WS05).
//!
//! Until a tool declares its class, execution-core guesses its risk from the
//! tool's name (`permission::is_risky_tool`), and capability-core carries one
//! `risk_level` for everything behind a capability. This module is the shared
//! vocabulary that replaces the guess: a closed set of classes, ordered from the
//! weakest to the strictest, and the approval floor a class may be run under.
//!
//! It lives in the contracts crate for the reason [`crate::autonomy`] does: the
//! registry (Go, which stores a class) and the gate (Rust, which reads one) must
//! agree on what a class means, and two copies of an ordering are two chances
//! for a class to mean something weaker on one side. The Go twin is
//! `capability-core/internal/models/tool_class.go`; both are pinned by the same
//! known answers (`known_answers` below and `TestSideEffectClassKnownAnswers`).
//!
//! # Unknown is strictest
//!
//! [`SideEffectClass::Unclassified`] ranks above every other class, and every
//! parse of a value outside the vocabulary lands on it. A tool nobody has
//! classified is treated as the most dangerous thing it could be, never as a
//! read, so adding a tool without declaring it can only add a pause.

use crate::model_plane::v1::AutonomyRung;

/// What a call to a tool may change. Ordered weakest to strictest: the derived
/// ordering is the strictness ordering, so [`SideEffectClass::stricter`] is `max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SideEffectClass {
    /// Observes only: no state changes anywhere a person would notice.
    Read,
    /// Computes inside the run's hermetic sandbox: no network, no host files,
    /// output scrubbed. Needs no approval, and is not a durable write.
    SandboxCompute,
    /// A durable write confined to the run's own context (the person's memory).
    ContextWrite,
    /// Acts on something outside the run: a provider account, a site, an order.
    ExternalWrite,
    /// Destroys or overwrites, or runs arbitrary host commands.
    Destructive,
    /// An effect only the owner plane may authorise, run once per approval.
    OwnerEffect,
    /// Nobody has classified it. The strictest class.
    Unclassified,
}

/// The least approval a class may be run under. A floor only ever tightens what
/// a posture would otherwise decide. Ordered weakest to strictest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ApprovalFloor {
    /// The posture alone decides.
    None,
    /// A person is asked on every posture except a stricter `deny`.
    Ask,
    /// A durable, bound human approval from the owner plane, single use.
    DurableOwner,
}

/// What a server's annotations amount to once the MCP interpretation defaults are
/// applied. `Undeclared` is its own value so silence is never mistaken for a
/// statement. Mirrors model-gateway's `McpSideEffect`, which this crate cannot
/// depend on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclaredSideEffect {
    /// `readOnlyHint: true`.
    ReadOnly,
    /// Not read-only, with `destructiveHint: false`: writes additively.
    NonDestructiveWrite,
    /// Not read-only, destructive by hint or by the spec's default.
    Destructive,
    /// The server said nothing usable.
    Undeclared,
}

/// The one tool whose class depends on its arguments: `execute_provider_action`
/// is a read or a write by its `operation`, so its row is the write class and
/// carries this refinement; a call whose operation is catalogued as a read may
/// lower to [`SideEffectClass::Read`], and nothing else may lower.
pub const CALL_REFINEMENT_PROVIDER_OPERATION: &str = "provider_operation";

impl SideEffectClass {
    /// Every class, weakest to strictest.
    pub const ALL: [Self; 7] = [
        Self::Read,
        Self::SandboxCompute,
        Self::ContextWrite,
        Self::ExternalWrite,
        Self::Destructive,
        Self::OwnerEffect,
        Self::Unclassified,
    ];

    /// The wire and storage name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::SandboxCompute => "sandbox_compute",
            Self::ContextWrite => "context_write",
            Self::ExternalWrite => "external_write",
            Self::Destructive => "destructive",
            Self::OwnerEffect => "owner_effect",
            Self::Unclassified => "unclassified",
        }
    }

    /// Parse a stored or wire value. Exact match only: an empty value, a
    /// different case or padding, and anything unknown are
    /// [`Self::Unclassified`], never a guess at the nearest class.
    #[must_use]
    pub fn from_wire(raw: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|class| class.as_str() == raw)
            .unwrap_or(Self::Unclassified)
    }

    /// The stricter of two classes.
    #[must_use]
    pub fn stricter(self, other: Self) -> Self {
        self.max(other)
    }

    /// Whether a call of this class needs the human gate under the `ask`
    /// posture. `Unclassified` is risky by construction.
    #[must_use]
    pub fn is_risky(self) -> bool {
        self >= Self::ExternalWrite
    }

    /// Whether a restricted context (plan mode, a delegated leaf) must refuse
    /// this class although it is not risky: a durable write confined to the
    /// run's own context.
    #[must_use]
    pub fn is_restricted_context_write(self) -> bool {
        self == Self::ContextWrite
    }

    /// The autonomy rung a call of this class needs.
    #[must_use]
    pub fn required_rung(self) -> AutonomyRung {
        match self {
            Self::Read | Self::SandboxCompute => AutonomyRung::ReadOnly,
            Self::ContextWrite => AutonomyRung::WorkspaceWrite,
            Self::ExternalWrite | Self::Destructive | Self::OwnerEffect | Self::Unclassified => {
                AutonomyRung::DangerFullAccess
            }
        }
    }

    /// The floor the class implies by itself, whatever a row says: an owner
    /// effect is never run without a durable owner approval, and a tool nobody
    /// has classified is never run without a person being asked, so an
    /// `Unclassified` row (the default every new row is born with) cannot run
    /// under an `auto` posture with no approval.
    #[must_use]
    pub fn minimum_floor(self) -> ApprovalFloor {
        match self {
            Self::OwnerEffect => ApprovalFloor::DurableOwner,
            Self::Unclassified => ApprovalFloor::Ask,
            Self::Read
            | Self::SandboxCompute
            | Self::ContextWrite
            | Self::ExternalWrite
            | Self::Destructive => ApprovalFloor::None,
        }
    }
}

impl ApprovalFloor {
    /// Every floor, weakest to strictest.
    pub const ALL: [Self; 3] = [Self::None, Self::Ask, Self::DurableOwner];

    /// The wire and storage name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Ask => "ask",
            Self::DurableOwner => "durable_owner",
        }
    }

    /// Parse a stored or wire value. Anything unrecognised is
    /// [`Self::DurableOwner`], the strictest floor, so a corrupt value can only
    /// add a pause.
    #[must_use]
    pub fn from_wire(raw: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|floor| floor.as_str() == raw)
            .unwrap_or(Self::DurableOwner)
    }

    /// The stricter of two floors.
    #[must_use]
    pub fn stricter(self, other: Self) -> Self {
        self.max(other)
    }
}

impl DeclaredSideEffect {
    /// The storage name, as written to `capability_tools.declared_side_effect`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::NonDestructiveWrite => "non_destructive_write",
            Self::Destructive => "destructive",
            Self::Undeclared => "undeclared",
        }
    }

    /// Parse a stored value; anything unrecognised is [`Self::Undeclared`].
    #[must_use]
    pub fn from_wire(raw: &str) -> Self {
        [Self::ReadOnly, Self::NonDestructiveWrite, Self::Destructive]
            .into_iter()
            .find(|declared| declared.as_str() == raw)
            .unwrap_or(Self::Undeclared)
    }
}

/// The class of an MCP tool, derived from what its server declared. Derivation
/// only tightens. A server's annotations are third-party claims, so unless the
/// org has chosen to trust them a `ReadOnly` claim is not believed and derives as
/// an external write. A server that said nothing derives as `Unclassified`, and a
/// destructive claim is believed in both modes because believing it can only add
/// a pause.
#[must_use]
pub fn derive_mcp_class(
    declared: DeclaredSideEffect,
    annotations_trusted: bool,
) -> SideEffectClass {
    match declared {
        DeclaredSideEffect::ReadOnly if annotations_trusted => SideEffectClass::Read,
        DeclaredSideEffect::ReadOnly | DeclaredSideEffect::NonDestructiveWrite => {
            SideEffectClass::ExternalWrite
        }
        DeclaredSideEffect::Destructive => SideEffectClass::Destructive,
        DeclaredSideEffect::Undeclared => SideEffectClass::Unclassified,
    }
}

/// The floor to enforce for a stored (class, floor) pair: the stored floor raised
/// to what the class implies. A caller that reads only this can never see an
/// owner effect without its durable owner approval.
#[must_use]
pub fn effective_floor(class: SideEffectClass, floor: ApprovalFloor) -> ApprovalFloor {
    floor.stricter(class.minimum_floor())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autonomy;

    /// The known answers shared with the Go twin
    /// (`TestSideEffectClassKnownAnswers`): rank, risky, restricted context
    /// write, rung, and the floor the class implies by itself (`unclassified`
    /// implies `ask`, `owner_effect` implies `durable_owner`). Kept identical by
    /// hand.
    #[test]
    fn known_answers() {
        let vectors: [(&str, usize, bool, bool, &str, &str); 7] = [
            ("read", 0, false, false, "read_only", "none"),
            ("sandbox_compute", 1, false, false, "read_only", "none"),
            ("context_write", 2, false, true, "workspace_write", "none"),
            (
                "external_write",
                3,
                true,
                false,
                "danger_full_access",
                "none",
            ),
            ("destructive", 4, true, false, "danger_full_access", "none"),
            (
                "owner_effect",
                5,
                true,
                false,
                "danger_full_access",
                "durable_owner",
            ),
            ("unclassified", 6, true, false, "danger_full_access", "ask"),
        ];
        for (index, (name, rank, risky, restricted, rung, min_floor)) in
            vectors.into_iter().enumerate()
        {
            let class = SideEffectClass::from_wire(name);
            assert_eq!(class, SideEffectClass::ALL[index], "{name}");
            assert_eq!(class.as_str(), name);
            assert_eq!(
                SideEffectClass::ALL.iter().position(|c| *c == class),
                Some(rank)
            );
            assert_eq!(class.is_risky(), risky, "{name} risky");
            assert_eq!(
                class.is_restricted_context_write(),
                restricted,
                "{name} restricted"
            );
            assert_eq!(autonomy::label(class.required_rung()), rung, "{name} rung");
            assert_eq!(
                class.minimum_floor().as_str(),
                min_floor,
                "{name} minimum floor"
            );
            // A stored `none` floor reads as the class's own minimum, never below.
            assert_eq!(
                effective_floor(class, ApprovalFloor::None).as_str(),
                min_floor,
                "{name} effective floor of a stored none"
            );
            // An empty floor is the strictest floor for every class, the same
            // answer the Go twin's EffectiveApprovalFloor(class, "") gives.
            assert_eq!(
                effective_floor(class, ApprovalFloor::from_wire("")),
                ApprovalFloor::DurableOwner,
                "{name} effective floor of an empty floor"
            );
        }
    }

    #[test]
    fn unknown_wire_values_are_the_strictest_class() {
        for raw in [
            "", "READ", "Read", " read", "read ", "unknown", "readonly", "none",
        ] {
            let class = SideEffectClass::from_wire(raw);
            assert_eq!(class, SideEffectClass::Unclassified, "{raw:?}");
            assert!(class.is_risky(), "{raw:?} must be risky");
            assert_eq!(class.required_rung(), AutonomyRung::DangerFullAccess);
        }
        for raw in ["", "ASK", "owner", "durable", "required"] {
            assert_eq!(
                ApprovalFloor::from_wire(raw),
                ApprovalFloor::DurableOwner,
                "{raw:?}"
            );
        }
        for (index, floor) in ApprovalFloor::ALL.into_iter().enumerate() {
            assert_eq!(ApprovalFloor::from_wire(floor.as_str()), floor);
            assert_eq!(ApprovalFloor::ALL[index], floor);
        }
    }

    #[test]
    fn stricter_is_the_higher_rank() {
        use SideEffectClass::{Destructive, OwnerEffect, Read, Unclassified};
        assert_eq!(Read.stricter(Destructive), Destructive);
        assert_eq!(Destructive.stricter(Read), Destructive);
        assert_eq!(OwnerEffect.stricter(Unclassified), Unclassified);
        assert_eq!(
            SideEffectClass::from_wire("nonsense").stricter(Read),
            Unclassified
        );
        assert_eq!(
            ApprovalFloor::None.stricter(ApprovalFloor::Ask),
            ApprovalFloor::Ask
        );
        assert_eq!(
            ApprovalFloor::DurableOwner.stricter(ApprovalFloor::Ask),
            ApprovalFloor::DurableOwner
        );
    }

    #[test]
    fn an_owner_effect_always_carries_the_durable_owner_floor() {
        assert_eq!(
            SideEffectClass::OwnerEffect.minimum_floor(),
            ApprovalFloor::DurableOwner
        );
        assert_eq!(
            effective_floor(SideEffectClass::OwnerEffect, ApprovalFloor::None),
            ApprovalFloor::DurableOwner
        );
        assert_eq!(
            effective_floor(SideEffectClass::Read, ApprovalFloor::Ask),
            ApprovalFloor::Ask
        );
        assert_eq!(
            effective_floor(SideEffectClass::Read, ApprovalFloor::None),
            ApprovalFloor::None
        );
    }

    /// A tool nobody has classified is never reported below an ask, so the
    /// default every new row is born with cannot run unasked under `auto`.
    #[test]
    fn an_unclassified_class_never_reads_below_an_ask() {
        assert_eq!(
            SideEffectClass::Unclassified.minimum_floor(),
            ApprovalFloor::Ask
        );
        assert_eq!(
            effective_floor(SideEffectClass::Unclassified, ApprovalFloor::None),
            ApprovalFloor::Ask
        );
        assert_eq!(
            effective_floor(SideEffectClass::from_wire("nonsense"), ApprovalFloor::None),
            ApprovalFloor::Ask
        );
        assert_eq!(
            effective_floor(SideEffectClass::Unclassified, ApprovalFloor::DurableOwner),
            ApprovalFloor::DurableOwner
        );
    }

    /// The same table as `TestDeriveMCPClassOnlyTightens` in the Go twin.
    #[test]
    fn mcp_derivation_only_tightens() {
        use DeclaredSideEffect::{Destructive, NonDestructiveWrite, ReadOnly, Undeclared};
        let vectors = [
            (ReadOnly, true, SideEffectClass::Read),
            // A read-only claim from a server nobody chose to trust is a
            // third-party claim, not evidence.
            (ReadOnly, false, SideEffectClass::ExternalWrite),
            (NonDestructiveWrite, true, SideEffectClass::ExternalWrite),
            (NonDestructiveWrite, false, SideEffectClass::ExternalWrite),
            (Destructive, true, SideEffectClass::Destructive),
            (Destructive, false, SideEffectClass::Destructive),
            (Undeclared, true, SideEffectClass::Unclassified),
            (Undeclared, false, SideEffectClass::Unclassified),
        ];
        for (declared, trusted, want) in vectors {
            assert_eq!(
                derive_mcp_class(declared, trusted),
                want,
                "{declared:?} trusted={trusted}"
            );
        }
        assert_eq!(DeclaredSideEffect::from_wire("surprise"), Undeclared);
        assert_eq!(DeclaredSideEffect::from_wire(""), Undeclared);
        for declared in [ReadOnly, NonDestructiveWrite, Destructive, Undeclared] {
            let round_trip = DeclaredSideEffect::from_wire(declared.as_str());
            // `Undeclared` is the parse of everything unrecognised, including
            // its own name.
            assert_eq!(round_trip, declared);
        }
    }
}
