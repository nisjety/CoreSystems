//! The org's `ext.skills` setting, applied to what a Do run injects and recovers (P2-ec-01).
//!
//! The setting (`none` | `approved_only` | `any`) is the org's, and execution-core has no policy of its own and
//! never reads org-core. The gateway states it on the run (`RunAgentRequest.ext_skills`, a policy word, never skill
//! text) from the policy the turn was admitted under, only while its `SKILL_EXT_POLICY_ENFORCED` switch is the
//! literal `true`; the empty word means "not enforced" and changes nothing. Execution-core acts only on a non-empty
//! word, so with the gateway switch off every Do run is byte for byte what it was.
//!
//! The word is read once where the run starts and held in a small in-process registry keyed `(org_id, run_id)`
//! for the run's life ([`RunExtSkills::register`], dropped with its guard), because `reattach_skill` and the site
//! skill matcher run deep inside the tool dispatch with no request in scope. A Do-managed run with no entry
//! (a restart, another replica) fails closed: it answers the typed error [`UNAVAILABLE`] and reads nothing.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, PoisonError};

use mp_contracts::model_plane::v1 as pb;

/// The typed tool error a Do run answers when the org's policy for it cannot be seen (no entry for the run).
pub(crate) const UNAVAILABLE: &str = "skill_policy_unavailable";

/// What the org's `ext.skills` lets a Do run use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExtSkills {
    /// The gateway did not enforce the setting (empty word): every skill the caller may use, as before.
    NotEnforced,
    /// No authored skill at all.
    None,
    /// The org's own skills only: a person's skill and a Space's skill are not used.
    ApprovedOnly,
    /// Every skill the caller may use.
    Any,
}

impl ExtSkills {
    /// Reads the run's policy word. Fail closed: a non-empty word that is neither `approved_only` nor `any` is
    /// `none`, so an unreadable word can only narrow. Only the exact empty word is "not enforced".
    #[must_use]
    pub(crate) fn from_token(token: &str) -> Self {
        match token {
            "" => Self::NotEnforced,
            "any" => Self::Any,
            "approved_only" => Self::ApprovedOnly,
            _ => Self::None,
        }
    }

    /// Whether this setting lets the skill reach a run. The skill's tier is its `scope`: `org` is the org tier,
    /// anything else (`user`, `space`, an unknown word) is the person tier, as the gateway classifies it.
    #[must_use]
    pub(crate) fn permits(self, skill: &pb::AgentSkill) -> bool {
        match self {
            Self::NotEnforced | Self::Any => true,
            Self::ApprovedOnly => skill.scope.eq_ignore_ascii_case("org"),
            Self::None => false,
        }
    }

    /// The skills this setting lets through, in order.
    #[must_use]
    pub(crate) fn filter(self, skills: Vec<pb::AgentSkill>) -> Vec<pb::AgentSkill> {
        skills
            .into_iter()
            .filter(|skill| self.permits(skill))
            .collect()
    }
}

#[derive(Debug, Default)]
struct Held {
    next_generation: u64,
    runs: HashMap<(String, String), (u64, ExtSkills)>,
}

/// The `ext.skills` each running Do run was started under.
#[derive(Debug, Default)]
pub(crate) struct RunExtSkills {
    held: Mutex<Held>,
}

/// Removes the run's entry when the run loop that registered it ends, by any path. An entry a later registration
/// of the same run replaced is left alone.
#[derive(Debug)]
#[must_use = "the run's skill policy entry is removed when the guard drops"]
pub(crate) struct RunExtSkillsGuard {
    registry: &'static RunExtSkills,
    key: (String, String),
    generation: u64,
}

impl Drop for RunExtSkillsGuard {
    fn drop(&mut self) {
        let mut held = self
            .registry
            .held
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if held
            .runs
            .get(&self.key)
            .is_some_and(|(generation, _)| *generation == self.generation)
        {
            held.runs.remove(&self.key);
        }
    }
}

/// The process-wide registry.
pub(crate) fn run_ext_skills() -> &'static RunExtSkills {
    static REGISTRY: OnceLock<RunExtSkills> = OnceLock::new();
    REGISTRY.get_or_init(RunExtSkills::default)
}

impl RunExtSkills {
    /// Holds the run's policy word until the guard drops. A continuation registers its own request's word.
    pub(crate) fn register(
        &'static self,
        org_id: &str,
        run_id: &str,
        token: &str,
    ) -> RunExtSkillsGuard {
        let key = (org_id.to_owned(), run_id.to_owned());
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        held.next_generation += 1;
        let generation = held.next_generation;
        held.runs
            .insert(key.clone(), (generation, ExtSkills::from_token(token)));
        RunExtSkillsGuard {
            registry: self,
            key,
            generation,
        }
    }

    /// What the run's tool dispatch applies: the setting the run started under; for a run with no entry,
    /// `Err` when the run is Do-managed (it fails closed, [`UNAVAILABLE`]) and "not enforced" otherwise, so a
    /// single-step call that is no Do run reads as it always did.
    pub(crate) fn for_run(
        &self,
        org_id: &str,
        run_id: &str,
        do_managed: bool,
    ) -> Result<ExtSkills, &'static str> {
        let held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        match held.runs.get(&(org_id.to_owned(), run_id.to_owned())) {
            Some((_, ext)) => Ok(*ext),
            None if do_managed => Err(UNAVAILABLE),
            None => Ok(ExtSkills::NotEnforced),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(name: &str, scope: &str) -> pb::AgentSkill {
        pb::AgentSkill {
            id: format!("sk-{name}"),
            name: name.to_owned(),
            content: format!("how to {name}"),
            enabled: true,
            scope: scope.to_owned(),
            ..Default::default()
        }
    }

    fn rows() -> Vec<pb::AgentSkill> {
        vec![
            skill("org-row", "org"),
            skill("user-row", "user"),
            skill("space-row", "space"),
            skill("no-scope-row", ""),
        ]
    }

    fn names(skills: Vec<pb::AgentSkill>) -> Vec<String> {
        skills.into_iter().map(|skill| skill.name).collect()
    }

    fn leaked() -> &'static RunExtSkills {
        Box::leak(Box::new(RunExtSkills::default()))
    }

    #[test]
    fn the_matrix_none_approved_only_and_any_over_org_user_and_space_rows() {
        assert!(names(ExtSkills::from_token("none").filter(rows())).is_empty());
        assert_eq!(
            names(ExtSkills::from_token("approved_only").filter(rows())),
            ["org-row"],
            "only the org tier; a person's, a Space's and an unscoped row are not used"
        );
        assert_eq!(
            names(ExtSkills::from_token("any").filter(rows())),
            ["org-row", "user-row", "space-row", "no-scope-row"]
        );
        assert!(
            ExtSkills::ApprovedOnly.permits(&skill("x", "ORG")),
            "the scope word is case-blind"
        );
    }

    #[test]
    fn the_empty_word_is_not_enforced_and_changes_nothing() {
        assert_eq!(ExtSkills::from_token(""), ExtSkills::NotEnforced);
        assert_eq!(
            names(ExtSkills::from_token("").filter(rows())),
            ["org-row", "user-row", "space-row", "no-scope-row"]
        );
    }

    #[test]
    fn an_unknown_word_fails_closed_to_none() {
        for word in [
            "approved",
            "ANY",
            " any",
            "any ",
            "approved-only",
            "true",
            "none",
        ] {
            assert_eq!(ExtSkills::from_token(word), ExtSkills::None, "{word:?}");
            assert!(
                names(ExtSkills::from_token(word).filter(rows())).is_empty(),
                "{word:?}"
            );
        }
    }

    #[test]
    fn a_registered_run_reads_its_word_and_the_guard_drops_it() {
        let registry = leaked();
        let guard = registry.register("org-1", "run-1", "approved_only");
        assert_eq!(
            registry.for_run("org-1", "run-1", true),
            Ok(ExtSkills::ApprovedOnly)
        );
        // Another org with the same run id is another run.
        assert_eq!(registry.for_run("org-2", "run-1", true), Err(UNAVAILABLE));
        drop(guard);
        assert_eq!(registry.for_run("org-1", "run-1", true), Err(UNAVAILABLE));
    }

    #[test]
    fn a_do_run_with_no_entry_fails_closed_and_a_single_step_reads_as_it_always_did() {
        let registry = leaked();
        assert_eq!(
            registry.for_run("org-1", "run-gone", true),
            Err(UNAVAILABLE)
        );
        assert_eq!(
            registry.for_run("org-1", "run-gone", false),
            Ok(ExtSkills::NotEnforced)
        );
    }

    #[test]
    fn a_registration_that_replaced_an_older_one_survives_the_older_guard() {
        let registry = leaked();
        let first = registry.register("org-1", "run-1", "none");
        let second = registry.register("org-1", "run-1", "any");
        drop(first);
        assert_eq!(registry.for_run("org-1", "run-1", true), Ok(ExtSkills::Any));
        drop(second);
        assert_eq!(registry.for_run("org-1", "run-1", true), Err(UNAVAILABLE));
    }
}
