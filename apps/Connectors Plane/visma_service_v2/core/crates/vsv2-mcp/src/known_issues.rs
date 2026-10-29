//! Known data issues, shown by `visma_init` so a model learns them at the start of a session instead
//! of after it has reported a wrong figure. Each entry is dated, says how serious it is, and points at
//! the skill section that explains it and how to check whether it still holds. Tests tie every entry
//! to a real skill and a phrase that really appears in it, so the list cannot drift from the guidance.

use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::{json, Value};

const RAW: &str = include_str!("../known_issues.json");

/// What goes wrong if the issue is ignored.
pub const SEVERITIES: &[&str] = &["wrong_totals", "double_counting", "check_before_reporting"];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnownIssue {
    pub id: String,
    /// ISO date it was found (`YYYY-MM-DD`); an issue is a statement about that day.
    pub found: String,
    pub severity: String,
    pub summary: String,
    /// The skill that explains it and how to check whether it still holds.
    pub skill: String,
    /// A phrase that must appear in that skill, so the pointer cannot dangle.
    pub anchor: String,
}

pub fn all() -> &'static [KnownIssue] {
    static ISSUES: OnceLock<Vec<KnownIssue>> = OnceLock::new();
    ISSUES.get_or_init(|| {
        serde_json::from_str(RAW).unwrap_or_else(|e| panic!("known_issues.json is invalid: {e}"))
    })
}

/// What each severity means for an answer that ignores the issue.
fn severity_legend() -> Value {
    json!({
        "wrong_totals": "a figure you would report is wrong (too high or too low) unless you correct for it",
        "double_counting": "the same money is counted twice if you add the sources it appears in",
        "check_before_reporting": "the data is not wrong as such, but you must name the source and the limit when you report it",
    })
}

/// What `visma_init` returns: the list plus how to treat it.
pub fn as_json() -> Value {
    json!({
        "note": "Each issue was true on the date it was found. Read its skill for how to check whether it still \
                 holds before relying on it, and say so when an answer is affected.",
        "severity_legend": severity_legend(),
        "issues": all().iter().map(|i| json!({
            "id": i.id, "found": i.found, "severity": i.severity, "summary": i.summary, "skill": i.skill,
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::skills::SkillIndex;

    fn is_iso_date(s: &str) -> bool {
        let b = s.as_bytes();
        b.len() == 10
            && b[4] == b'-'
            && b[7] == b'-'
            && s.char_indices()
                .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
            && (1..=12).contains(&s[5..7].parse::<u32>().unwrap_or(0))
            && (1..=31).contains(&s[8..10].parse::<u32>().unwrap_or(0))
    }

    #[test]
    fn the_list_parses_and_every_entry_is_well_formed() {
        let issues = all();
        assert!(!issues.is_empty());
        let mut ids = BTreeSet::new();
        for i in issues {
            assert!(ids.insert(i.id.as_str()), "duplicate id {}", i.id);
            assert!(
                i.id.split('-').all(|p| !p.is_empty()
                    && p.chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())),
                "{}: id must be kebab-case",
                i.id
            );
            assert!(
                is_iso_date(&i.found),
                "{}: found must be YYYY-MM-DD, got {}",
                i.id,
                i.found
            );
            assert!(
                SEVERITIES.contains(&i.severity.as_str()),
                "{}: unknown severity {}",
                i.id,
                i.severity
            );
            assert!(
                (60..=500).contains(&i.summary.len()),
                "{}: summary should be one short paragraph ({} chars)",
                i.id,
                i.summary.len()
            );
        }
    }

    #[test]
    fn every_issue_points_at_a_skill_that_really_explains_it() {
        let index = SkillIndex::embedded();
        for i in all() {
            let skill = index
                .get(&i.skill)
                .unwrap_or_else(|| panic!("{}: skill {} does not exist", i.id, i.skill));
            assert!(
                skill.body.contains(&i.anchor),
                "{}: skill {} no longer contains the phrase {:?}",
                i.id,
                i.skill,
                i.anchor
            );
        }
    }

    #[test]
    fn every_severity_in_use_is_explained_in_the_legend() {
        let legend = severity_legend();
        for sev in SEVERITIES {
            assert!(
                legend[sev].as_str().is_some_and(|t| t.len() > 20),
                "no legend for {sev}"
            );
        }
        for i in all() {
            assert!(
                legend.get(&i.severity).is_some(),
                "{} uses an unexplained severity",
                i.id
            );
        }
    }

    #[test]
    fn the_json_for_init_carries_the_caveat_and_every_issue() {
        let v = as_json();
        assert!(v["note"].as_str().unwrap().contains("still"));
        assert_eq!(v["issues"].as_array().unwrap().len(), all().len());
        assert!(v["issues"][0]["skill"].is_string());
    }
}
