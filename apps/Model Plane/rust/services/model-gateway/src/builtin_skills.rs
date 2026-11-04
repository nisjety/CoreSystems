//! First-party Office skills (WS19-S9).
//!
//! Four SKILL.md files teach the model to build .docx, .xlsx, .pptx and .pdf files
//! in the `code_interpreter` sandbox. They are part of the gateway build, not data
//! of any org: with `BUILTIN_OFFICE_SKILLS_ENABLED=true` every org's catalogue lists
//! them (see `SkillStore::list`), and a learned refresh cannot remove them, because
//! they are never stored per org.
//!
//! The execution-core image copies the same `skills/office` directory to
//! `/opt/verevon/skills/office`, where the sandbox can read a skill's files. The
//! `builtin_skills_contract` test ties the embedded text to those files.

use std::sync::OnceLock;

use mp_contracts::model_plane::v1::Skill;

/// Environment switch. Only the literal `true` enables it, so an unset, empty or
/// mistyped value leaves every catalogue as it was.
pub const BUILTIN_OFFICE_SKILLS_ENV: &str = "BUILTIN_OFFICE_SKILLS_ENABLED";

/// Catalogue id and `source_path` prefix of every first-party skill. A skill that
/// carries it is found by its name and trigger keywords, not by its body (see
/// `skills::score`).
pub const SOURCE_PREFIX: &str = "builtin:office/";

/// One first-party skill as the gateway binary holds it.
pub struct BuiltinSkill {
    /// Directory under `skills/office`; the last part of the catalogue id.
    pub dir: &'static str,
    /// The name the model sees, which the file's front matter declares too.
    pub name: &'static str,
    /// Words a request uses for this skill. A request word counts only when it is a
    /// substring of the name or of a keyword (`skills::score` does not stem), so list
    /// the plural and local-language forms users type.
    pub trigger_keywords: &'static [&'static str],
    /// The SKILL.md as it is in the repository.
    pub text: &'static str,
}

pub const OFFICE_SKILLS: [BuiltinSkill; 4] = [
    BuiltinSkill {
        dir: "docx",
        name: "Word (docx)",
        trigger_keywords: &[
            "docx",
            "word document",
            "word documents",
            "letter",
            "letters",
            "brev",
            "memorandum",
            "referat",
        ],
        text: include_str!("../../../../skills/office/docx/SKILL.md"),
    },
    BuiltinSkill {
        dir: "xlsx",
        name: "Excel (xlsx)",
        trigger_keywords: &[
            "xlsx",
            "excel",
            "spreadsheet",
            "spreadsheets",
            "workbook",
            "workbooks",
            "regneark",
            "budget",
            "budgets",
            "budsjett",
            "pivot",
        ],
        text: include_str!("../../../../skills/office/xlsx/SKILL.md"),
    },
    BuiltinSkill {
        dir: "pptx",
        name: "PowerPoint (pptx)",
        trigger_keywords: &[
            "pptx",
            "powerpoint",
            "presentation",
            "presentations",
            "presentasjon",
            "presentasjoner",
            "slide",
            "slides",
            "deck",
            "decks",
            "lysark",
        ],
        text: include_str!("../../../../skills/office/pptx/SKILL.md"),
    },
    BuiltinSkill {
        dir: "pdf",
        name: "PDF (pdf)",
        trigger_keywords: &["pdf", "pdfs"],
        text: include_str!("../../../../skills/office/pdf/SKILL.md"),
    },
];

/// Whether the switch is on: only the literal `true` enables it.
#[must_use]
pub fn enabled_from(raw: Option<&str>) -> bool {
    raw == Some("true")
}

/// Whether the first-party Office skills are listed for every org in this process.
#[must_use]
pub fn enabled() -> bool {
    enabled_from(std::env::var(BUILTIN_OFFICE_SKILLS_ENV).ok().as_deref())
}

/// The first-party skills as the match cache holds a skill. Built once: they are
/// compiled in, so they cannot change while the process runs.
#[must_use]
pub fn skills() -> Vec<Skill> {
    static SKILLS: OnceLock<Vec<Skill>> = OnceLock::new();
    SKILLS
        .get_or_init(|| OFFICE_SKILLS.iter().map(BuiltinSkill::to_skill).collect())
        .clone()
}

/// The first-party skill with this catalogue id, if there is one.
#[must_use]
pub fn skill(id: &str) -> Option<Skill> {
    skills().into_iter().find(|skill| skill.id == id)
}

/// Whether a catalogue entry is one of the first-party skills.
#[must_use]
pub fn is_builtin(skill: &Skill) -> bool {
    skill.source_path.starts_with(SOURCE_PREFIX)
}

impl BuiltinSkill {
    fn to_skill(&self) -> Skill {
        let id = format!("{SOURCE_PREFIX}{}", self.dir);
        Skill {
            id: id.clone(),
            name: self.name.to_owned(),
            body: skill_body(self.text),
            tags: self
                .trigger_keywords
                .iter()
                .map(|keyword| (*keyword).to_owned())
                .collect(),
            source_path: id,
            min_score: 0.0,
            // A builtin has no numbered versions: the binary is its version, and the
            // embedded text's hash names it (EP-11 S3). Carried only with pins on, so
            // a listing stays the one sent before while they are off.
            version: 0,
            digest: if crate::skills::skill_pins_enabled() {
                text_digest(self.text)
            } else {
                String::new()
            },
        }
    }
}

/// Lowercase hex sha256 of a SKILL.md with CRLF folded to LF, so the digest does
/// not depend on the checkout's line endings (EP-11 S3).
fn text_digest(text: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.replace("\r\n", "\n").as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The digest a run pins for the first-party skill with this catalogue id: the
/// hash of its embedded SKILL.md. `None` for any other id.
#[must_use]
pub fn digest_of(id: &str) -> Option<String> {
    let dir = id.strip_prefix(SOURCE_PREFIX)?;
    OFFICE_SKILLS
        .iter()
        .find(|skill| skill.dir == dir)
        .map(|skill| text_digest(skill.text))
}

/// The instruction the model reads: the file without its front matter, which is for
/// people and the skill format. CRLF is folded to LF, so the prompt does not depend
/// on the checkout's line endings.
fn skill_body(text: &str) -> String {
    const FENCE: &str = "\n---\n";
    let normalized = text.replace("\r\n", "\n");
    let body = normalized
        .strip_prefix("---\n")
        .and_then(|rest| rest.find(FENCE).map(|end| &rest[end + FENCE.len()..]))
        .unwrap_or(normalized.as_str());
    body.trim_start().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ownership::OwnershipStore;
    use crate::skills::{
        handle_list_skills, handle_match_skills, resolve_requested_skills, SkillStore,
        SKILL_CONTEXT_BUDGET_CHARS,
    };
    use mp_contracts::model_plane::v1::{AgentSkill, ListSkillsRequest, MatchSkillsRequest};

    /// A store built with the switch on. The field is private to `skills`, which is
    /// this module's parent, so the test can set it without an environment variable.
    fn with_builtins() -> SkillStore {
        SkillStore {
            builtin_office: true,
            ..SkillStore::default()
        }
    }

    fn listed_ids(store: &SkillStore, ownership: &OwnershipStore, org_id: &str) -> Vec<String> {
        let mut ids: Vec<String> = handle_list_skills(
            store,
            ListSkillsRequest {
                request_id: "t".into(),
                org_id: org_id.into(),
            },
            ownership,
            "alice",
        )
        .expect("list")
        .skills
        .into_iter()
        .map(|skill| skill.id)
        .collect();
        ids.sort();
        ids
    }

    fn matched_ids(store: &SkillStore, query: &str) -> Vec<String> {
        handle_match_skills(
            store,
            MatchSkillsRequest {
                request_id: "t".into(),
                org_id: "o".into(),
                query: query.into(),
                limit: 0,
                min_score: 0.0,
            },
            &OwnershipStore::new(),
            "alice",
        )
        .expect("match")
        .matches
        .into_iter()
        .filter_map(|m| m.skill.map(|skill| skill.id))
        .collect()
    }

    fn all_builtin_ids() -> Vec<String> {
        let mut ids: Vec<String> = OFFICE_SKILLS
            .iter()
            .map(|skill| format!("{SOURCE_PREFIX}{}", skill.dir))
            .collect();
        ids.sort();
        ids
    }

    fn learned(id: &str, name: &str) -> AgentSkill {
        AgentSkill {
            id: id.into(),
            name: name.into(),
            content: format!("learned body of {name}"),
            enabled: true,
            origin: "background_review".into(),
            scope: "org".into(),
            review_state: "accepted".into(),
            ..Default::default()
        }
    }

    #[test]
    fn builtin_skills_are_registered_for_every_org() {
        let store = with_builtins();
        let ownership = OwnershipStore::new();
        for org_id in ["org-a", "org-b"] {
            for caller in ["alice", "bob"] {
                let listed = handle_list_skills(
                    &store,
                    ListSkillsRequest {
                        request_id: "t".into(),
                        org_id: org_id.into(),
                    },
                    &ownership,
                    caller,
                )
                .expect("list");
                let mut ids: Vec<String> = listed.skills.into_iter().map(|s| s.id).collect();
                ids.sort();
                assert_eq!(ids, all_builtin_ids(), "{org_id} as {caller}");
            }
        }
    }

    #[test]
    fn a_learned_refresh_keeps_builtin_skills() {
        let store = with_builtins();
        let ownership = OwnershipStore::new();
        store.refresh_learned(&ownership, "o", "alice", vec![learned("s1", "house style")]);
        assert_eq!(listed_ids(&store, &ownership, "o").len(), 5);

        // The learned skill is deleted upstream: the refresh prunes it and nothing else.
        store.refresh_learned(&ownership, "o", "alice", vec![]);
        assert_eq!(listed_ids(&store, &ownership, "o"), all_builtin_ids());
    }

    #[test]
    fn a_request_finds_the_skill_by_its_trigger_keywords() {
        let store = with_builtins();
        assert_eq!(
            matched_ids(&store, "make an Excel budget"),
            vec!["builtin:office/xlsx".to_owned()]
        );
    }

    #[test]
    fn a_builtin_is_not_matched_by_words_only_its_body_has() {
        // Every body is English instruction prose with "the", "for" and "with" in it.
        // None of these request words is in a name or a keyword.
        let store = with_builtins();
        assert!(matched_ids(
            &store,
            "the team meeting notes about the plan for next week"
        )
        .is_empty());
    }

    #[test]
    fn the_switch_reads_only_the_literal_true() {
        assert!(enabled_from(Some("true")));
        for off in [
            None,
            Some(""),
            Some("TRUE"),
            Some(" true "),
            Some("1"),
            Some("yes"),
            Some("false"),
        ] {
            assert!(!enabled_from(off), "{off:?} must leave the switch off");
        }
    }

    #[test]
    fn a_store_built_without_the_switch_lists_no_builtin() {
        let store = SkillStore::default();
        assert!(listed_ids(&store, &OwnershipStore::new(), "o").is_empty());
        assert!(matched_ids(&store, "make an Excel budget").is_empty());
    }

    #[test]
    fn a_stored_skill_under_a_builtin_id_is_listed_once() {
        let store = with_builtins();
        store.upsert(
            "o",
            Skill {
                id: "builtin:office/xlsx".into(),
                name: "Our Excel rules".into(),
                body: "house rules".into(),
                tags: Vec::new(),
                source_path: String::new(),
                min_score: 0.0,
                version: 0,
                digest: String::new(),
            },
        );
        assert_eq!(
            listed_ids(&store, &OwnershipStore::new(), "o"),
            all_builtin_ids()
        );
        let listed = handle_list_skills(
            &store,
            ListSkillsRequest {
                request_id: "t".into(),
                org_id: "o".into(),
            },
            &OwnershipStore::new(),
            "alice",
        )
        .expect("list");
        assert!(listed
            .skills
            .iter()
            .any(|skill| skill.name == "Our Excel rules"));
    }

    #[test]
    fn an_explicit_pick_resolves_a_builtin_by_its_id() {
        let store = with_builtins();
        let picked = resolve_requested_skills(
            &store,
            "o",
            &OwnershipStore::new(),
            "alice",
            &["builtin:office/pdf".to_owned()],
            4,
        );
        let ids: Vec<&str> = picked.iter().map(|skill| skill.id.as_str()).collect();
        assert_eq!(ids, vec!["builtin:office/pdf"]);
    }

    #[test]
    fn the_front_matter_is_not_part_of_the_instruction() {
        assert_eq!(
            skill_body("---\nname: x\n---\n# Title\nbody\n"),
            "# Title\nbody\n"
        );
        assert_eq!(
            skill_body("---\r\nname: x\r\n---\r\n# Title\r\n"),
            "# Title\n"
        );
        assert_eq!(skill_body("# No front matter\n"), "# No front matter\n");
        assert_eq!(
            skill_body("---\nname: x\nno closing fence\n"),
            "---\nname: x\nno closing fence\n"
        );
    }

    #[test]
    fn each_body_fits_a_third_of_the_skill_budget() {
        // Three matched skills share the prompt budget with their headings. A body
        // over a third of it would be cut whenever all three match.
        for skill in skills() {
            assert!(
                skill.body.chars().count() <= SKILL_CONTEXT_BUDGET_CHARS / 3 - 100,
                "{} is too long for the shared skill budget",
                skill.name
            );
        }
    }
}
