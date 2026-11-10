//! The `verevon-role` resolver (EP-13 S4). A side task names the role it plays
//! instead of a model, and the platform role table picks the model. The chain then
//! runs the picked model as an ordinary pinned call, so every gate that applies to a
//! pinned call applies to it as well.

use mp_contracts::model_plane::v1::InferPurpose;

use super::effort::Level;
use super::routing_policy::{RolePurpose, RoutingPolicy};

/// The model id a side task names to ask the role table. The gateway names the same
/// id from its own side (`model_gateway::role_routing`), and the two services are
/// deployed apart, so a change here is a change there too.
pub const ROLE_MODEL_ID: &str = "verevon-role";

/// Where the chosen model came from, in the order the resolver tries them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleSource {
    /// A candidate of the caller's organisation's own table entry (W4-BYO-3), which is tried before the
    /// platform table.
    OrgTable,
    /// A candidate of the purpose's table entry.
    Table,
    /// The turn's own model, which the gateway names as the fallback.
    TurnModel,
    /// The policy's cheap fallback, the last resort.
    CheapFallback,
}

impl RoleSource {
    /// The source's name as the route note spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OrgTable => "org_table",
            Self::Table => "table",
            Self::TurnModel => "turn_model",
            Self::CheapFallback => "cheap_fallback",
        }
    }
}

/// A `verevon-role` request's role: the purpose it serves and the model its turn was
/// served by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleCall {
    /// The purpose the role table serves. `None` when the request names no such
    /// purpose, and the chain refuses the call.
    pub purpose: Option<RolePurpose>,
    /// The turn's own served model, the candidate after the table's entries. Empty
    /// when the caller names none.
    pub fallback_model: String,
}

impl RoleCall {
    /// Reads the role off the wire fields a `verevon-role` request carries.
    #[must_use]
    pub fn from_wire(purpose: i32, fallback_model: &str) -> Self {
        Self {
            purpose: role_purpose(purpose),
            fallback_model: fallback_model.to_owned(),
        }
    }
}

/// The role table's purpose for a wire purpose, or `None` for one the table does not serve.
fn role_purpose(purpose: i32) -> Option<RolePurpose> {
    match InferPurpose::try_from(purpose).ok()? {
        InferPurpose::Title => Some(RolePurpose::Title),
        InferPurpose::FollowUps => Some(RolePurpose::FollowUps),
        InferPurpose::VerificationJudge => Some(RolePurpose::VerificationJudge),
        InferPurpose::ResearchPlan => Some(RolePurpose::ResearchPlan),
        _ => None,
    }
}

/// What the route note records about a role decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoleDecision {
    /// The purpose whose table entry chose the model. `None` when no entry did.
    pub table_entry: Option<RolePurpose>,
    pub source: RoleSource,
}

/// The model a role call runs on, and the decision that picked it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleChoice {
    pub model: String,
    pub decision: RoleDecision,
    /// The effort the table entry sets, when the entry chose the model.
    pub effort: Option<Level>,
}

/// Picks the model for a role call (EP-13 D7). The purpose's table candidates come
/// first, in order, then the turn's own model, then the policy's cheap fallback.
/// `admits` says whether a candidate may serve this call, and it is asked only of the
/// table candidates and the turn's model. The cheap fallback is taken without the
/// question: the chain's own gates then refuse it when it cannot serve the call, so a
/// call that nothing can serve still ends in the usual typed refusal, never in a
/// silent pick.
#[must_use]
pub fn choose(
    policy: &RoutingPolicy,
    purpose: RolePurpose,
    turn_model: &str,
    admits: impl Fn(&str) -> bool,
) -> RoleChoice {
    choose_with_org(policy, &[], purpose, turn_model, admits)
}

/// [`choose`] with the caller's organisation's candidates for the purpose tried first (W4-BYO-3). `org_candidates`
/// is the organisation's narrowed list for this purpose, already limited to ids the platform catalogue lists, and
/// each one is still asked of `admits`, so the entitlement, ZDR, privacy-tier and residency floors apply to it as
/// they do to a platform candidate. An organisation candidate sets no effort: the effort is the platform entry's,
/// and it applies only when the platform entry picks the model. An empty list is [`choose`] exactly.
#[must_use]
pub fn choose_with_org(
    policy: &RoutingPolicy,
    org_candidates: &[String],
    purpose: RolePurpose,
    turn_model: &str,
    admits: impl Fn(&str) -> bool,
) -> RoleChoice {
    if let Some(model) = org_candidates.iter().find(|candidate| admits(candidate)) {
        return RoleChoice {
            model: model.clone(),
            decision: RoleDecision {
                table_entry: Some(purpose),
                source: RoleSource::OrgTable,
            },
            effort: None,
        };
    }
    if let Some(entry) = policy.roles.get(&purpose) {
        if let Some(model) = entry.candidates.iter().find(|candidate| admits(candidate)) {
            return RoleChoice {
                model: model.clone(),
                decision: RoleDecision {
                    table_entry: Some(purpose),
                    source: RoleSource::Table,
                },
                effort: entry.effort.as_deref().and_then(Level::from_name),
            };
        }
    }
    if !turn_model.trim().is_empty() && admits(turn_model) {
        return RoleChoice {
            model: turn_model.to_owned(),
            decision: RoleDecision {
                table_entry: None,
                source: RoleSource::TurnModel,
            },
            effort: None,
        };
    }
    RoleChoice {
        model: policy.cheap_fallback.clone(),
        decision: RoleDecision {
            table_entry: None,
            source: RoleSource::CheapFallback,
        },
        effort: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::routing_policy::RoleEntry;

    fn table(purpose: RolePurpose, candidates: &[&str], effort: Option<&str>) -> RoutingPolicy {
        let mut policy = RoutingPolicy::default();
        policy.roles.insert(
            purpose,
            RoleEntry {
                candidates: candidates.iter().map(|id| (*id).to_owned()).collect(),
                effort: effort.map(str::to_owned),
            },
        );
        policy
    }

    #[test]
    fn only_the_side_task_purposes_name_a_role() {
        assert_eq!(
            RoleCall::from_wire(InferPurpose::Title as i32, "").purpose,
            Some(RolePurpose::Title)
        );
        assert_eq!(
            RoleCall::from_wire(InferPurpose::ResearchPlan as i32, "").purpose,
            Some(RolePurpose::ResearchPlan)
        );
        assert_eq!(RoleCall::from_wire(InferPurpose::Answer as i32, "").purpose, None);
        assert_eq!(RoleCall::from_wire(InferPurpose::Unspecified as i32, "").purpose, None);
        assert_eq!(RoleCall::from_wire(999, "").purpose, None);
    }

    #[test]
    fn first_admitted_candidate_wins() {
        let policy = table(RolePurpose::Title, &["gone", "gpt-5-nano", "gpt-4o-mini"], None);
        // "gone" is not admitted (the chain does not list it), so the next candidate wins.
        let choice = choose(&policy, RolePurpose::Title, "claude-sonnet-5-5", |model| {
            model != "gone"
        });
        assert_eq!(choice.model, "gpt-5-nano");
        assert_eq!(
            choice.decision,
            RoleDecision {
                table_entry: Some(RolePurpose::Title),
                source: RoleSource::Table,
            }
        );
    }

    #[test]
    fn empty_table_uses_turn_model() {
        let choice = choose(
            &RoutingPolicy::default(),
            RolePurpose::Title,
            "claude-sonnet-5-5",
            |_| true,
        );
        assert_eq!(choice.model, "claude-sonnet-5-5");
        assert_eq!(
            choice.decision,
            RoleDecision {
                table_entry: None,
                source: RoleSource::TurnModel,
            }
        );
    }

    #[test]
    fn a_refused_turn_model_gives_way_to_the_untested_cheap_fallback() {
        let choice = choose(
            &RoutingPolicy::default(),
            RolePurpose::Title,
            "claude-sonnet-5-5",
            |_| false,
        );
        assert_eq!(choice.model, RoutingPolicy::default().cheap_fallback);
        assert_eq!(choice.decision.source, RoleSource::CheapFallback);
        // An empty turn model is never a candidate, so the cheap fallback answers.
        let empty = choose(&RoutingPolicy::default(), RolePurpose::Title, "  ", |_| true);
        assert_eq!(empty.decision.source, RoleSource::CheapFallback);
    }

    #[test]
    fn table_effort_applies_only_when_the_entry_chooses_the_model() {
        let policy = table(RolePurpose::Title, &["gpt-5-nano"], Some("minimal"));
        let by_table = choose(&policy, RolePurpose::Title, "claude-sonnet-5-5", |_| true);
        assert_eq!(by_table.effort, Some(Level::Minimal));
        let by_turn = choose(&policy, RolePurpose::Title, "claude-sonnet-5-5", |model| {
            model != "gpt-5-nano"
        });
        assert_eq!(by_turn.effort, None);
    }

    #[test]
    fn route_note_names_role_and_source() {
        assert_eq!(RoleSource::Table.as_str(), "table");
        assert_eq!(RoleSource::TurnModel.as_str(), "turn_model");
        assert_eq!(RoleSource::CheapFallback.as_str(), "cheap_fallback");
        // Only a table choice names its purpose; the other two sources name none.
        let policy = table(RolePurpose::FollowUps, &["gpt-5-nano"], None);
        let by_table = choose(&policy, RolePurpose::FollowUps, "", |_| true);
        assert_eq!(by_table.decision.table_entry, Some(RolePurpose::FollowUps));
        let by_turn = choose(&RoutingPolicy::default(), RolePurpose::FollowUps, "m", |_| true);
        assert_eq!(by_turn.decision.table_entry, None);
    }
}
