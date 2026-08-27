//! The autonomy ladder, and what makes a request to widen it well-formed.
//!
//! Lives in the contracts crate rather than in either service because both need
//! the *same* rules and neither may depend on the other: model-gateway decides
//! and records a grant, execution-core enforces it per call. Two copies of an
//! ordering is two chances for a grant to mean something wider on one side than
//! the other.
//!
//! # Adapted from `deepseek-harness` (MIT)
//!
//! The three-rung ladder, the strictly-wider escalation table, and the rule that
//! a request carries a justification are its design; the rung names are our own
//! `MpSandboxPolicy` vocabulary, so a granted rung and the isolation it implies
//! cannot drift apart. Its reasoning, which this module encodes:
//!
//! * *An approval prompt without a reason, or a reason driving nothing, is a
//!   malformed ask.* So [`AutonomyEscalation::request`] refuses both — an empty
//!   justification, and a "widening" that widens nothing.
//! * *Schemas are registry-global while the effective mode is per-call truth.*
//!   So nothing here belongs in a tool schema; the check is applied at
//!   execution, per call.
//! * *Oversized handoffs fail rather than being silently truncated.* A truncated
//!   justification reads as a complete one, so an oversized one is refused.

use crate::model_plane::v1::AutonomyRung;

/// Longest accepted justification. Generous for a sentence or two of reasoning,
/// short enough that it cannot become a transcript smuggled through an approval
/// record.
pub const MAX_JUSTIFICATION_CHARS: usize = 1_000;

/// Shortest accepted justification.
///
/// Not zero-plus-one: "ok", "yes" and "-" are all non-empty and all say nothing,
/// and a required field that accepts anything is a required field in name only.
pub const MIN_JUSTIFICATION_CHARS: usize = 12;

/// Position on the ladder. Higher is strictly wider.
///
/// `UNSPECIFIED` deliberately ranks with the NARROWEST rung rather than being
/// rejected outright, so an older caller that never set the field is treated as
/// having no authority instead of having all of it. Rejecting it would also be
/// defensible; reading it as a grant would not.
#[must_use]
pub fn rank(rung: AutonomyRung) -> u8 {
    match rung {
        AutonomyRung::Unspecified | AutonomyRung::ReadOnly => 0,
        AutonomyRung::WorkspaceWrite => 1,
        AutonomyRung::DangerFullAccess => 2,
    }
}

/// Whether a run at `granted` may make a call that requires `needed`.
///
/// The per-call check. Called at execution with the run's granted rung and the
/// rung the specific call requires — never consulted when building a tool
/// schema, because a schema is registry-global while this answer is per call.
#[must_use]
pub fn permits(granted: AutonomyRung, needed: AutonomyRung) -> bool {
    rank(granted) >= rank(needed)
}

/// Why a request to widen autonomy was refused.
///
/// Each variant is a distinct thing the requester must do differently, which is
/// the whole reason this is not a bool: "say why" and "you already have this"
/// call for opposite next moves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EscalationRefusal {
    /// The target is not wider than the current rung.
    NotAnEscalation {
        from: AutonomyRung,
        to: AutonomyRung,
    },
    /// No target rung was named.
    NoTargetRung,
    /// Missing, or too thin to be a reason.
    JustificationTooShort { chars: usize },
    /// Long enough that it is a document, not a reason.
    JustificationTooLong { chars: usize },
}

impl EscalationRefusal {
    /// The sentence the requester reads. Names the fix, not just the fault —
    /// a refusal a model cannot act on becomes a retry loop against the same
    /// wall.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::NotAnEscalation { from, to } => format!(
                "not an escalation: {} is not wider than {}, which this run already has. \
                 Request a wider rung, or proceed with what you have.",
                label(*to),
                label(*from)
            ),
            Self::NoTargetRung => {
                "no rung requested: name the authority you need (workspace_write or \
                 danger_full_access), because an approval that grants nothing specific \
                 grants everything by omission."
                    .to_owned()
            }
            Self::JustificationTooShort { chars } => format!(
                "justification is too short ({chars} characters, minimum \
                 {MIN_JUSTIFICATION_CHARS}): state what you need to do and why the narrower \
                 rung cannot do it. A person is being asked to approve this."
            ),
            Self::JustificationTooLong { chars } => format!(
                "justification is too long ({chars} characters, maximum \
                 {MAX_JUSTIFICATION_CHARS}): give the reason, not the plan. It is refused \
                 rather than shortened, because a truncated reason reads as a complete one."
            ),
        }
    }
}

/// The wire-and-log name of a rung.
#[must_use]
pub fn label(rung: AutonomyRung) -> &'static str {
    match rung {
        AutonomyRung::Unspecified => "unspecified",
        AutonomyRung::ReadOnly => "read_only",
        AutonomyRung::WorkspaceWrite => "workspace_write",
        AutonomyRung::DangerFullAccess => "danger_full_access",
    }
}

/// Parse a rung from a caller-supplied keyword, accepting `_` or `-` and any
/// case. Returns `None` for anything unrecognised — never a default, because
/// guessing a rung from an unknown word is how a typo becomes a grant.
#[must_use]
pub fn rung_from_keyword(keyword: &str) -> Option<AutonomyRung> {
    match keyword
        .trim()
        .to_ascii_lowercase()
        .replace('-', "_")
        .as_str()
    {
        "read_only" => Some(AutonomyRung::ReadOnly),
        "workspace_write" => Some(AutonomyRung::WorkspaceWrite),
        "danger_full_access" => Some(AutonomyRung::DangerFullAccess),
        _ => None,
    }
}

/// A validated request to widen a run's autonomy.
///
/// Constructible only through [`request`](Self::request), so a malformed ask
/// cannot exist as a value — the validation is not a step a caller can forget to
/// run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutonomyEscalation {
    from: AutonomyRung,
    to: AutonomyRung,
    justification: String,
}

impl AutonomyEscalation {
    /// Validate a request to move from `from` to `to`, because `justification`.
    ///
    /// # Errors
    ///
    /// Returns the specific [`EscalationRefusal`] rather than a bool: each one
    /// tells the requester a different thing to do next.
    pub fn request(
        from: AutonomyRung,
        to: AutonomyRung,
        justification: &str,
    ) -> Result<Self, EscalationRefusal> {
        if to == AutonomyRung::Unspecified {
            return Err(EscalationRefusal::NoTargetRung);
        }
        if rank(to) <= rank(from) {
            return Err(EscalationRefusal::NotAnEscalation { from, to });
        }
        let trimmed = justification.trim();
        let chars = trimmed.chars().count();
        if chars < MIN_JUSTIFICATION_CHARS {
            return Err(EscalationRefusal::JustificationTooShort { chars });
        }
        if chars > MAX_JUSTIFICATION_CHARS {
            return Err(EscalationRefusal::JustificationTooLong { chars });
        }
        Ok(Self {
            from,
            to,
            justification: trimmed.to_owned(),
        })
    }

    /// The rung being left.
    #[must_use]
    pub fn from(&self) -> AutonomyRung {
        self.from
    }

    /// The rung being granted.
    #[must_use]
    pub fn to(&self) -> AutonomyRung {
        self.to
    }

    /// The stated reason, trimmed.
    #[must_use]
    pub fn justification(&self) -> &str {
        &self.justification
    }

    /// One line for an audit record or an approval prompt. Carries both the
    /// grant and its reason, because either alone is unreviewable.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} -> {}: {}",
            label(self.from),
            label(self.to),
            self.justification
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REASON: &str = "the plan needs to write the generated report to the workspace";

    #[test]
    fn the_ladder_is_strictly_ordered() {
        assert!(rank(AutonomyRung::ReadOnly) < rank(AutonomyRung::WorkspaceWrite));
        assert!(rank(AutonomyRung::WorkspaceWrite) < rank(AutonomyRung::DangerFullAccess));
    }

    /// THE fail-closed property. An older caller that never sets the field sends
    /// `UNSPECIFIED`; reading that as a grant would silently authorize
    /// everything on every request that predates the field.
    #[test]
    fn an_unset_rung_is_the_narrowest_never_a_grant() {
        assert_eq!(
            rank(AutonomyRung::Unspecified),
            rank(AutonomyRung::ReadOnly)
        );
        assert!(!permits(
            AutonomyRung::Unspecified,
            AutonomyRung::WorkspaceWrite
        ));
        assert!(!permits(
            AutonomyRung::Unspecified,
            AutonomyRung::DangerFullAccess
        ));
    }

    #[test]
    fn a_rung_permits_itself_and_everything_narrower() {
        assert!(permits(
            AutonomyRung::WorkspaceWrite,
            AutonomyRung::WorkspaceWrite
        ));
        assert!(permits(
            AutonomyRung::WorkspaceWrite,
            AutonomyRung::ReadOnly
        ));
        assert!(permits(
            AutonomyRung::DangerFullAccess,
            AutonomyRung::WorkspaceWrite
        ));
        assert!(!permits(
            AutonomyRung::WorkspaceWrite,
            AutonomyRung::DangerFullAccess
        ));
    }

    #[test]
    fn a_widening_request_with_a_real_reason_is_accepted() {
        let escalation = AutonomyEscalation::request(
            AutonomyRung::ReadOnly,
            AutonomyRung::WorkspaceWrite,
            REASON,
        )
        .expect("a wider rung with a stated reason is a well-formed ask");
        assert_eq!(escalation.to(), AutonomyRung::WorkspaceWrite);
        assert!(escalation
            .summary()
            .contains("read_only -> workspace_write"));
        assert!(
            escalation.summary().contains(REASON),
            "an audit line without the reason is unreviewable: {}",
            escalation.summary()
        );
    }

    /// "A reason driving nothing is a malformed ask." Sideways and downward
    /// requests are refused with the reason stated, not silently granted as
    /// no-ops — a no-op grant still exits plan mode.
    #[test]
    fn a_request_that_widens_nothing_is_refused() {
        for (from, to) in [
            (AutonomyRung::WorkspaceWrite, AutonomyRung::WorkspaceWrite),
            (AutonomyRung::DangerFullAccess, AutonomyRung::ReadOnly),
            (AutonomyRung::WorkspaceWrite, AutonomyRung::ReadOnly),
        ] {
            let refusal = AutonomyEscalation::request(from, to, REASON)
                .expect_err("{to:?} is not wider than {from:?}");
            assert_eq!(refusal, EscalationRefusal::NotAnEscalation { from, to });
            assert!(refusal.message().contains("not an escalation"));
        }
    }

    #[test]
    fn a_request_with_no_target_rung_is_refused() {
        let refusal =
            AutonomyEscalation::request(AutonomyRung::ReadOnly, AutonomyRung::Unspecified, REASON)
                .expect_err("an unnamed target grants everything by omission");
        assert_eq!(refusal, EscalationRefusal::NoTargetRung);
    }

    /// A required field that accepts "ok" is required in name only. The minimum
    /// exists because a person reads this before deciding.
    #[test]
    fn a_reason_that_says_nothing_is_refused_like_a_missing_one() {
        for thin in ["", "   ", "ok", "yes", "-", "needed"] {
            let refusal = AutonomyEscalation::request(
                AutonomyRung::ReadOnly,
                AutonomyRung::WorkspaceWrite,
                thin,
            )
            .expect_err("a non-reason must be refused: {thin:?}");
            assert!(
                matches!(refusal, EscalationRefusal::JustificationTooShort { .. }),
                "{thin:?} -> {refusal:?}"
            );
        }
    }

    /// Refused, not shortened. A truncated reason reads as a complete one, and
    /// the person approving cannot tell that the rest was cut.
    #[test]
    fn an_oversized_reason_fails_rather_than_being_truncated() {
        let long = "x".repeat(MAX_JUSTIFICATION_CHARS + 1);
        let refusal = AutonomyEscalation::request(
            AutonomyRung::ReadOnly,
            AutonomyRung::DangerFullAccess,
            &long,
        )
        .expect_err("an oversized justification is refused");
        assert_eq!(
            refusal,
            EscalationRefusal::JustificationTooLong {
                chars: MAX_JUSTIFICATION_CHARS + 1
            }
        );
        assert!(
            refusal.message().contains("rather than shortened"),
            "the refusal must say WHY it is not truncated: {}",
            refusal.message()
        );
    }

    /// Counted in characters, not bytes: a Norwegian justification at the
    /// boundary must not be refused for its diacritics.
    #[test]
    fn the_length_bounds_count_characters() {
        let at_limit = "æ".repeat(MAX_JUSTIFICATION_CHARS);
        assert!(AutonomyEscalation::request(
            AutonomyRung::ReadOnly,
            AutonomyRung::WorkspaceWrite,
            &at_limit
        )
        .is_ok());
    }

    /// An unrecognised keyword is `None`, never a rung. Guessing would turn a
    /// typo into a grant.
    #[test]
    fn an_unknown_rung_keyword_is_not_guessed_at() {
        assert_eq!(
            rung_from_keyword("WORKSPACE-WRITE"),
            Some(AutonomyRung::WorkspaceWrite)
        );
        assert_eq!(
            rung_from_keyword(" danger_full_access "),
            Some(AutonomyRung::DangerFullAccess)
        );
        for unknown in ["full", "write", "admin", "yolo", ""] {
            assert_eq!(rung_from_keyword(unknown), None, "{unknown:?}");
        }
    }
}
