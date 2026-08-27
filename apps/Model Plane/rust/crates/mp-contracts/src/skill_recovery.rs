//! Reading a truncated or dropped skill back in full.
//!
//! # The gap this closes
//!
//! Skill injection is bounded by a character budget that **degrades before
//! dropping**: a skill too long for the remaining room is cut and marked, and
//! one that cannot keep a useful minimum is dropped and counted. Both halves are
//! honest about having happened — and neither was recoverable. The model read
//! `[… skill truncated to fit the context budget]` and had no way to obtain the
//! rest, which is the same shape as the dropped-history notice before it learned
//! to name `reattach_context`: a notice that says "this is incomplete" without a
//! route to completeness.
//!
//! An instruction is the worst thing to leave half-read. The model acts on half
//! a rule believing it is whole, and the operator who wrote the rule cannot tell
//! from the answer that only its opening survived.
//!
//! # Why this lives in the contracts crate
//!
//! Both loops inject skills under the same budget and must recover them the same
//! way — `model-gateway::skills` and `execution-core::runtime_loop::skill_budget`
//! deliberately share the BUDGET while keeping their own block layout. Neither
//! service may depend on the other, and a second copy of the selection rule is a
//! second chance for one loop to recover a different skill than the other.

use crate::model_plane::v1::AgentSkill;

/// Longest skill body returned in one recovery.
///
/// Larger than the whole per-prompt skill budget on purpose: the point of asking
/// is to see the parts the budget cut. Still bounded, because an operator can
/// author a skill of any length and a recovery that dominates the prompt has
/// only moved the problem.
pub const MAX_RECOVERED_SKILL_CHARS: usize = 20_000;

/// What a recovery request resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveredSkill {
    /// The skill, in full or bounded to [`MAX_RECOVERED_SKILL_CHARS`].
    Found {
        name: String,
        description: String,
        content: String,
        /// True when even this bound cut the body. Stated rather than hidden,
        /// for exactly the reason this module exists.
        truncated: bool,
    },
    /// No skill by that name is available to this org. Named skills are
    /// suggested so a near-miss becomes a correction rather than a dead end.
    NotFound { available: Vec<String> },
    /// The skill exists but is disabled — a different fact from not existing,
    /// and the model must not present a disabled rule as a live one.
    Disabled { name: String },
}

/// Resolve a skill by name from the org's own skill list.
///
/// Matching is case-insensitive and whitespace-trimmed, because the name the
/// model reads back came out of a formatted prompt block; it is otherwise exact.
/// No fuzzy matching: handing back a *different* skill than the one asked for is
/// worse than handing back nothing, since the model cannot tell which it got.
#[must_use]
pub fn recover_skill(skills: &[AgentSkill], requested: &str) -> RecoveredSkill {
    let wanted = requested.trim().to_lowercase();
    let matched = skills
        .iter()
        .find(|skill| skill.name.trim().to_lowercase() == wanted);

    match matched {
        Some(skill) if !skill.enabled => RecoveredSkill::Disabled {
            name: skill.name.clone(),
        },
        Some(skill) => {
            let body = skill.content.trim();
            let truncated = body.chars().count() > MAX_RECOVERED_SKILL_CHARS;
            RecoveredSkill::Found {
                name: skill.name.clone(),
                description: skill.description.trim().to_owned(),
                content: body.chars().take(MAX_RECOVERED_SKILL_CHARS).collect(),
                truncated,
            }
        }
        None => RecoveredSkill::NotFound {
            // Only enabled skills are offered as alternatives: suggesting a
            // disabled one would invite a second failed call.
            available: skills
                .iter()
                .filter(|skill| skill.enabled && !skill.name.trim().is_empty())
                .map(|skill| skill.name.trim().to_owned())
                .collect(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(name: &str, content: &str, enabled: bool) -> AgentSkill {
        AgentSkill {
            id: format!("sk-{name}"),
            name: name.to_owned(),
            description: "how to do the thing".to_owned(),
            content: content.to_owned(),
            enabled,
            ..Default::default()
        }
    }

    #[test]
    fn a_named_skill_comes_back_in_full() {
        let skills = vec![skill("invoicing", "Always invoice in NOK.", true)];
        assert_eq!(
            recover_skill(&skills, "invoicing"),
            RecoveredSkill::Found {
                name: "invoicing".to_owned(),
                description: "how to do the thing".to_owned(),
                content: "Always invoice in NOK.".to_owned(),
                truncated: false,
            }
        );
    }

    /// The name the model asks with was read out of a formatted prompt block, so
    /// case and surrounding space are noise. Anything beyond that is not.
    #[test]
    fn the_name_match_forgives_case_and_space_and_nothing_else() {
        let skills = vec![skill("Invoicing", "body", true)];
        assert!(matches!(
            recover_skill(&skills, "  invoicing "),
            RecoveredSkill::Found { .. }
        ));
        // NOT fuzzy: returning a different skill than the one asked for is worse
        // than returning nothing, because the model cannot tell which it got.
        assert!(matches!(
            recover_skill(&skills, "invoice"),
            RecoveredSkill::NotFound { .. }
        ));
    }

    /// Disabled is not missing. A disabled rule presented as a live one is a
    /// rule the operator switched off still steering answers.
    #[test]
    fn a_disabled_skill_is_reported_as_disabled_not_as_missing() {
        let skills = vec![skill("retired", "old rule", false)];
        assert_eq!(
            recover_skill(&skills, "retired"),
            RecoveredSkill::Disabled {
                name: "retired".to_owned()
            }
        );
    }

    /// A miss suggests what exists, so a near-miss is one correction rather than
    /// a dead end — but never suggests a disabled skill, which would just invite
    /// a second failed call.
    #[test]
    fn a_miss_suggests_only_the_skills_that_could_actually_be_read() {
        let skills = vec![
            skill("invoicing", "a", true),
            skill("retired", "b", false),
            skill("shipping", "c", true),
        ];
        let RecoveredSkill::NotFound { available } = recover_skill(&skills, "nope") else {
            panic!("expected a miss");
        };
        assert_eq!(
            available,
            vec!["invoicing".to_owned(), "shipping".to_owned()]
        );
    }

    /// Bounded, and it SAYS it is bounded — the whole point of this module is
    /// that a silently cut instruction reads as a complete one.
    #[test]
    fn an_oversized_body_is_bounded_and_says_so() {
        let long = "x".repeat(MAX_RECOVERED_SKILL_CHARS + 100);
        let skills = vec![skill("huge", &long, true)];
        let RecoveredSkill::Found {
            content, truncated, ..
        } = recover_skill(&skills, "huge")
        else {
            panic!("expected a hit");
        };
        assert!(truncated);
        assert_eq!(content.chars().count(), MAX_RECOVERED_SKILL_CHARS);
    }

    /// Counted in characters: a Norwegian skill body at the boundary must not be
    /// cut early for its diacritics.
    #[test]
    fn the_bound_counts_characters() {
        let at_limit = "æ".repeat(MAX_RECOVERED_SKILL_CHARS);
        let skills = vec![skill("nordic", &at_limit, true)];
        let RecoveredSkill::Found { truncated, .. } = recover_skill(&skills, "nordic") else {
            panic!("expected a hit");
        };
        assert!(!truncated, "exactly at the limit is not over it");
    }

    #[test]
    fn an_empty_skill_list_is_a_miss_with_nothing_to_suggest() {
        assert_eq!(
            recover_skill(&[], "anything"),
            RecoveredSkill::NotFound { available: vec![] }
        );
    }
}
