//! Wave 10d — skills loader.
//!
//! v2 read .md skill files from disk (`auto_registry` + bundled). v1
//! keeps the same shape but the source-of-truth is an in-memory
//! registry — operators preload it at startup from disk or push
//! updates via gRPC. Skill bodies are markdown text shown to the
//! agent as system context when `MatchSkills` surfaces a relevant skill.

// tonic::Status is the unavoidable large Err for gRPC handlers; boxing breaks the service-trait contract.
#![allow(clippy::result_large_err)]

use std::sync::Arc;

use dashmap::DashMap;
use mp_ids::new_ulid;
use tonic::Status;

use mp_contracts::model_plane::v1::{
    AgentSkill, GetSkillRequest, GetSkillResponse, ListSkillsRequest, ListSkillsResponse,
    MatchSkillsRequest, MatchSkillsResponse, Skill, SkillMatch,
};

const DEFAULT_MATCH_LIMIT: i32 = 5;
const MAX_MATCH_LIMIT: i32 = 20;

/// Provenance prefix stamped on skills pulled from session-core, so a refresh
/// can replace exactly those without disturbing disk/operator-pushed skills.
/// Single source of truth for the two places that need to agree.
const LEARNED_SOURCE_PREFIX: &str = "session-core:";

/// How long an org's learned-skill pull stays fresh.
///
/// Was permanent: `loaded` recorded only *that* an org had been pulled, so a
/// skill edited or deleted through capability-core stayed invisible to a running
/// gateway until someone restarted it — which quietly contradicts the whole
/// point of a self-improving assistant whose operators author skills in the UI.
const DEFAULT_SKILL_CACHE_TTL_SECS: u64 = 60;

fn skill_cache_ttl() -> std::time::Duration {
    static TTL: std::sync::OnceLock<std::time::Duration> = std::sync::OnceLock::new();
    *TTL.get_or_init(|| {
        let secs = std::env::var("SKILL_CACHE_TTL_SECONDS")
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_SKILL_CACHE_TTL_SECS);
        std::time::Duration::from_secs(secs)
    })
}

#[derive(Clone, Default, Debug)]
pub struct SkillStore {
    // Keyed by (org_id, skill_id) so multi-tenant skill sets don't
    // bleed. Cheap clone (Arc<DashMap>).
    inner: Arc<DashMap<(String, String), Skill>>,
    // When each org's LEARNED skills (session-core agent_skills) were last
    // pulled via the §G7 lazy-load, so the pull is re-done on a TTL rather than
    // exactly once per process lifetime.
    loaded: Arc<DashMap<String, std::time::Instant>>,
}

impl SkillStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace a skill. Generates an id when empty.
    pub fn upsert(&self, org_id: &str, mut s: Skill) -> Skill {
        if s.id.is_empty() {
            s.id = new_ulid();
        }
        self.inner
            .insert((org_id.to_string(), s.id.clone()), s.clone());
        s
    }

    /// Whether this org's learned skills were pulled from session-core (G7 read
    /// path) recently enough to reuse. Goes stale on a TTL so an operator's edit
    /// or deletion takes effect without a gateway restart.
    #[must_use]
    pub fn is_org_loaded(&self, org_id: &str) -> bool {
        self.loaded
            .get(org_id)
            .is_some_and(|at| at.elapsed() < skill_cache_ttl())
    }

    /// Mark this org's learned skills as loaded (call after a successful pull).
    pub fn mark_org_loaded(&self, org_id: &str) {
        self.loaded
            .insert(org_id.to_owned(), std::time::Instant::now());
    }

    /// Make the cache match a fresh session-core pull exactly, then mark it
    /// loaded.
    ///
    /// Upserts first and prunes second, deliberately: evicting up front would
    /// leave a window where a concurrent turn matches against an org with no
    /// learned skills at all. Pruning by id is also what makes DELETION take
    /// effect — re-upserting alone would leave a removed skill steering answers
    /// forever. Only `session-core:`-sourced entries are touched, so
    /// disk/operator-pushed skills survive a refresh.
    pub fn replace_learned(&self, org_id: &str, skills: Vec<Skill>) {
        let mut fresh_ids: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for skill in skills {
            fresh_ids.insert(self.upsert(org_id, skill).id);
        }
        let stale: Vec<(String, String)> = self
            .inner
            .iter()
            .filter(|entry| {
                let (entry_org, skill_id) = entry.key();
                entry_org == org_id
                    && entry.value().source_path.starts_with(LEARNED_SOURCE_PREFIX)
                    && !fresh_ids.contains(skill_id)
            })
            .map(|entry| entry.key().clone())
            .collect();
        for key in stale {
            self.inner.remove(&key);
        }
        self.mark_org_loaded(org_id);
    }

    fn list(&self, org_id: &str) -> Vec<Skill> {
        let mut out: Vec<Skill> = self
            .inner
            .iter()
            .filter(|e| e.key().0 == org_id)
            .map(|e| e.value().clone())
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    fn get(&self, org_id: &str, skill_id: &str) -> Option<Skill> {
        self.inner
            .get(&(org_id.to_string(), skill_id.to_string()))
            .map(|s| s.value().clone())
    }
}

/// Lists all skills registered for `req.org_id` that `caller_user_id` may see
/// (SKILL-1): org-scoped and disk/operator-pushed skills (untracked in
/// `ownership`) are visible to everyone; a learned, user-scoped skill is
/// visible only to its owner or an explicit grantee.
///
/// # Errors
///
/// Returns `Status::invalid_argument` if `req.org_id` is empty.
pub fn handle_list_skills(
    store: &SkillStore,
    req: ListSkillsRequest,
    ownership: &crate::ownership::OwnershipStore,
    caller_user_id: &str,
) -> Result<ListSkillsResponse, Status> {
    if req.org_id.is_empty() {
        return Err(Status::invalid_argument("org_id is required"));
    }
    let skills = store
        .list(&req.org_id)
        .into_iter()
        .filter(|s| {
            ownership.usable_or_untracked(
                &req.org_id,
                crate::ownership::KIND_SKILL,
                &s.id,
                caller_user_id,
            )
        })
        .collect();
    Ok(ListSkillsResponse {
        request_id: req.request_id,
        skills,
    })
}

/// Fetches a single skill by id, same visibility rule as
/// [`handle_list_skills`]. Returns `not_found` rather than a permission error
/// when the caller may not see the skill, so existence isn't leaked.
///
/// # Errors
///
/// Returns `Status::not_found` if no skill matches `(org_id, skill_id)` or the
/// caller may not see it.
pub fn handle_get_skill(
    store: &SkillStore,
    req: GetSkillRequest,
    ownership: &crate::ownership::OwnershipStore,
    caller_user_id: &str,
) -> Result<GetSkillResponse, Status> {
    let s = store
        .get(&req.org_id, &req.skill_id)
        .ok_or_else(|| Status::not_found(format!("skill not found: {}", req.skill_id)))?;
    if !ownership.usable_or_untracked(
        &req.org_id,
        crate::ownership::KIND_SKILL,
        &s.id,
        caller_user_id,
    ) {
        return Err(Status::not_found(format!(
            "skill not found: {}",
            req.skill_id
        )));
    }
    Ok(GetSkillResponse {
        request_id: req.request_id,
        skill: Some(s),
    })
}

/// Map a session-core [`AgentSkill`] (the durable learned-skill row, G7) into a
/// gateway match-cache [`Skill`]. The body is the skill content; the trigger
/// keywords become the match tags; `source_path` records provenance so learned
/// skills are distinguishable from disk-loaded ones. The gateway `Skill` has no
/// description field, so `description` is intentionally not carried (the body
/// holds the substance used for matching).
#[must_use]
pub fn agent_skill_to_skill(a: AgentSkill) -> Skill {
    Skill {
        id: a.id,
        name: a.name,
        body: a.content,
        tags: a.trigger_keywords,
        source_path: format!("{LEARNED_SOURCE_PREFIX}{}", a.origin),
        min_score: 0.0,
    }
}

/// Derive an [`Ownership`](crate::ownership::Ownership) record from a
/// session-core [`AgentSkill`]'s `scope`/`owner_user_id`/`shared_with`
/// fields (SKILL-1), keyed by skill id — the shape
/// [`OwnershipStore::replace_org_kind`](crate::ownership::OwnershipStore::replace_org_kind)
/// expects on every G7 refresh.
#[must_use]
pub fn skill_ownership_entries(
    skills: &[AgentSkill],
) -> Vec<(String, crate::ownership::Ownership)> {
    skills
        .iter()
        .map(|a| {
            let ownership = match crate::ownership::Scope::from_wire(&a.scope) {
                crate::ownership::Scope::Org => crate::ownership::Ownership::org(),
                crate::ownership::Scope::User => crate::ownership::Ownership {
                    scope: crate::ownership::Scope::User,
                    owner_user_id: a.owner_user_id.clone(),
                    shared_with: a.shared_with.clone(),
                },
            };
            (a.id.clone(), ownership)
        })
        .collect()
}

/// Format a matched skill as a system-context block for live prompt injection.
/// This is the shape the SSE chat path prepends so a triggered skill steers the
/// model (Claude-Code skill semantics).
#[must_use]
pub fn format_skill_block(s: &Skill) -> String {
    format!("## Skill: {}\n{}", s.name, s.body)
}

/// Keyword overlap score. Lowercases everything and counts how many
/// of the query's tokens appear in the skill's name/tags/body. Cheap
/// and good enough for the < 100 skills regime; promote to BM25 if
/// catalogue grows.
fn score(query_terms: &[String], s: &Skill) -> f32 {
    if query_terms.is_empty() {
        return 0.0;
    }
    let haystack = format!("{} {} {}", s.name, s.tags.join(" "), s.body).to_lowercase();
    let hits = query_terms
        .iter()
        .filter(|t| haystack.contains(t.as_str()))
        .count();
    // reason: keyword-overlap ratio; small counts (< 100 terms) lose no meaningful precision in f32.
    #[allow(clippy::cast_precision_loss)]
    let ratio = hits as f32 / query_terms.len() as f32;
    ratio
}

/// Ranks an org's skills by keyword overlap with the query, restricted to
/// skills `caller_user_id` may see (SKILL-1, same rule as
/// [`handle_list_skills`]) — filtered before scoring/truncation so a
/// caller's `limit` matches are chosen from skills they're actually allowed
/// to see, not backfilled short by ones filtered out afterward.
///
/// # Errors
///
/// Returns `Status::invalid_argument` if `req.org_id` is empty.
pub fn handle_match_skills(
    store: &SkillStore,
    req: MatchSkillsRequest,
    ownership: &crate::ownership::OwnershipStore,
    caller_user_id: &str,
) -> Result<MatchSkillsResponse, Status> {
    if req.org_id.is_empty() {
        return Err(Status::invalid_argument("org_id is required"));
    }
    let query_terms: Vec<String> = req
        .query
        .to_lowercase()
        .split_whitespace()
        .filter(|t| t.len() >= 3) // skip stop-word-ish short tokens
        .map(str::to_string)
        .collect();

    let limit = usize::try_from(if req.limit <= 0 {
        DEFAULT_MATCH_LIMIT
    } else {
        req.limit.min(MAX_MATCH_LIMIT)
    })
    .unwrap_or(0);

    let mut scored: Vec<SkillMatch> = store
        .list(&req.org_id)
        .into_iter()
        .filter(|s| {
            ownership.usable_or_untracked(
                &req.org_id,
                crate::ownership::KIND_SKILL,
                &s.id,
                caller_user_id,
            )
        })
        .map(|s| {
            let match_score = score(&query_terms, &s);
            SkillMatch {
                skill: Some(s),
                score: match_score,
            }
        })
        .filter(|m| {
            let threshold = if req.min_score > 0.0 {
                req.min_score
            } else {
                m.skill.as_ref().map_or(0.0, |s| s.min_score)
            };
            m.score > 0.0 && m.score >= threshold
        })
        .collect();
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(limit);

    Ok(MatchSkillsResponse {
        request_id: req.request_id,
        matches: scored,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(name: &str, tags: &[&str], body: &str) -> Skill {
        Skill {
            id: String::new(),
            name: name.into(),
            body: body.into(),
            tags: tags.iter().map(std::string::ToString::to_string).collect(),
            source_path: String::new(),
            min_score: 0.0,
        }
    }

    #[test]
    fn upsert_assigns_id_when_empty() {
        let store = SkillStore::new();
        let out = store.upsert("o", s("test", &["a"], "body"));
        assert!(!out.id.is_empty());
    }

    #[test]
    fn org_loaded_tracking() {
        let store = SkillStore::new();
        assert!(!store.is_org_loaded("o"));
        store.mark_org_loaded("o");
        assert!(store.is_org_loaded("o"));
        assert!(!store.is_org_loaded("other"), "tracking is per-org");
    }

    /// A learned skill as it arrives from session-core, with the provenance
    /// prefix that makes it eligible for refresh-pruning.
    fn learned(id: &str, name: &str) -> Skill {
        Skill {
            id: id.into(),
            name: name.into(),
            body: format!("body of {name}"),
            tags: vec![],
            source_path: format!("{LEARNED_SOURCE_PREFIX}learned"),
            min_score: 0.0,
        }
    }

    #[test]
    fn stale_org_is_repulled_so_operator_edits_land_without_a_restart() {
        // The regression: `loaded` was a permanent marker, so an org was pulled
        // exactly once per process and later edits were invisible until restart.
        let store = SkillStore::new();
        store.loaded.insert(
            "o".to_owned(),
            std::time::Instant::now() - skill_cache_ttl(),
        );

        assert!(
            !store.is_org_loaded("o"),
            "a cache entry older than the TTL must read as stale"
        );
    }

    #[test]
    fn replace_learned_reflects_an_edited_skill_body() {
        let store = SkillStore::new();
        store.replace_learned("o", vec![learned("s1", "invoice-policy")]);
        store.replace_learned(
            "o",
            vec![Skill {
                body: "EDITED body".into(),
                ..learned("s1", "invoice-policy")
            }],
        );

        let listed = store.list("o");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].body, "EDITED body");
    }

    #[test]
    fn replace_learned_forgets_a_deleted_skill() {
        // Re-upserting alone would leave a deleted skill steering answers forever.
        let store = SkillStore::new();
        store.replace_learned(
            "o",
            vec![learned("s1", "keep-me"), learned("s2", "delete-me")],
        );
        store.replace_learned("o", vec![learned("s1", "keep-me")]);

        let names: Vec<String> = store.list("o").into_iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["keep-me".to_owned()], "deleted skill lingered");
    }

    #[test]
    fn replace_learned_leaves_operator_pushed_skills_alone() {
        // Disk/operator-pushed skills have no session-core provenance and must
        // survive a learned-skill refresh.
        let store = SkillStore::new();
        let pushed = store.upsert("o", s("operator-skill", &["ops"], "pushed body"));
        store.replace_learned("o", vec![learned("s1", "learned-skill")]);
        store.replace_learned("o", vec![]);

        let names: Vec<String> = store.list("o").into_iter().map(|s| s.name).collect();
        assert!(
            names.contains(&"operator-skill".to_owned()),
            "refresh clobbered a non-learned skill: {names:?}"
        );
        assert!(store.get("o", &pushed.id).is_some());
    }

    #[test]
    fn replace_learned_does_not_touch_another_org() {
        let store = SkillStore::new();
        store.replace_learned("o", vec![learned("s1", "mine")]);
        store.replace_learned("other", vec![learned("s9", "theirs")]);
        store.replace_learned("other", vec![]);

        assert_eq!(
            store.list("o").len(),
            1,
            "sibling org's refresh bled across"
        );
    }

    #[test]
    fn agent_skill_maps_into_match_cache_skill() {
        let a = AgentSkill {
            id: "sk-1".into(),
            name: "Cache Tips".into(),
            description: "ignored by the gateway Skill shape".into(),
            content: "use the cache".into(),
            trigger_keywords: vec!["cache".into(), "perf".into()],
            trigger_file_patterns: vec![],
            tool_restrictions: vec![],
            enabled: true,
            origin: "background_review".into(),
            scope: "org".into(),
            owner_user_id: String::new(),
            shared_with: vec![],
        };
        let sk = agent_skill_to_skill(a);
        assert_eq!(sk.id, "sk-1");
        assert_eq!(sk.name, "Cache Tips");
        assert_eq!(sk.body, "use the cache"); // content -> body
        assert_eq!(sk.tags, vec!["cache".to_owned(), "perf".to_owned()]); // keywords -> tags
        assert_eq!(sk.source_path, "session-core:background_review"); // provenance marker
                                                                      // A learned skill, once mapped, is matchable by its keywords.
        let store = SkillStore::new();
        store.upsert("o", sk);
        let ownership = crate::ownership::OwnershipStore::new();
        let resp = handle_match_skills(
            &store,
            MatchSkillsRequest {
                request_id: "r".into(),
                org_id: "o".into(),
                query: "help with cache".into(),
                limit: 0,
                min_score: 0.0,
            },
            &ownership,
            "caller",
        )
        .unwrap();
        assert!(
            resp.matches
                .iter()
                .any(|m| m.skill.as_ref().is_some_and(|s| s.name == "Cache Tips")),
            "learned skill should be matchable after mapping+upsert"
        );
    }

    #[test]
    fn format_skill_block_renders_name_and_body() {
        let block = format_skill_block(&s("Deploy Runbook", &["deploy"], "1. build\n2. ship"));
        assert_eq!(block, "## Skill: Deploy Runbook\n1. build\n2. ship");
    }

    #[test]
    fn list_filters_by_org() {
        let store = SkillStore::new();
        store.upsert("o1", s("A", &[], ""));
        store.upsert("o2", s("B", &[], ""));
        assert_eq!(store.list("o1").len(), 1);
        assert_eq!(store.list("o2").len(), 1);
    }

    #[test]
    fn match_ranks_keyword_overlap() {
        let store = SkillStore::new();
        store.upsert("o", s("python testing", &["pytest"], "Run pytest"));
        store.upsert("o", s("rust formatting", &["rustfmt"], "Run cargo fmt"));
        let ownership = crate::ownership::OwnershipStore::new();
        let resp = handle_match_skills(
            &store,
            MatchSkillsRequest {
                request_id: "t".into(),
                org_id: "o".into(),
                query: "how do I run pytest".into(),
                limit: 0,
                min_score: 0.0,
            },
            &ownership,
            "caller",
        )
        .unwrap();
        assert!(!resp.matches.is_empty());
        assert!(resp.matches[0]
            .skill
            .as_ref()
            .unwrap()
            .name
            .contains("python"));
    }

    #[test]
    fn match_hides_a_private_learned_skill_from_a_non_owner_non_grantee() {
        let store = SkillStore::new();
        let private = store.upsert("o", s("alice's runbook", &["private"], "secret steps"));
        let ownership = crate::ownership::OwnershipStore::new();
        ownership.set(
            "o",
            crate::ownership::KIND_SKILL,
            &private.id,
            crate::ownership::Ownership::user("alice"),
        );

        let req = MatchSkillsRequest {
            request_id: "t".into(),
            org_id: "o".into(),
            query: "private".into(),
            limit: 0,
            min_score: 0.0,
        };

        let owner_resp = handle_match_skills(&store, req.clone(), &ownership, "alice").unwrap();
        assert!(
            !owner_resp.matches.is_empty(),
            "owner must see their own skill"
        );

        let stranger_resp = handle_match_skills(&store, req, &ownership, "bob").unwrap();
        assert!(
            stranger_resp.matches.is_empty(),
            "a non-owner, non-grantee must not see a private learned skill"
        );
    }

    #[test]
    fn list_and_get_are_visible_to_untracked_and_owner_but_not_a_stranger() {
        let store = SkillStore::new();
        store.upsert("o", s("disk skill", &[], "always visible"));
        let private = store.upsert("o", s("private skill", &[], "owner only"));
        let ownership = crate::ownership::OwnershipStore::new();
        ownership.set(
            "o",
            crate::ownership::KIND_SKILL,
            &private.id,
            crate::ownership::Ownership::user("alice"),
        );

        let list_req = ListSkillsRequest {
            request_id: "t".into(),
            org_id: "o".into(),
        };
        let listed_for_owner =
            handle_list_skills(&store, list_req.clone(), &ownership, "alice").unwrap();
        assert_eq!(listed_for_owner.skills.len(), 2, "owner sees both");
        let listed_for_stranger = handle_list_skills(&store, list_req, &ownership, "bob").unwrap();
        assert_eq!(
            listed_for_stranger.skills.len(),
            1,
            "stranger sees only the untracked disk skill"
        );

        let get_req = GetSkillRequest {
            request_id: "t".into(),
            org_id: "o".into(),
            skill_id: private.id.clone(),
        };
        assert!(handle_get_skill(&store, get_req.clone(), &ownership, "alice").is_ok());
        assert!(handle_get_skill(&store, get_req, &ownership, "bob").is_err());
    }
}

/// Character budget for injected skill guidance — the chat loop's half of
/// plan item 3.5.
///
/// Skill injection was capped by COUNT (`sse::MAX_INJECTED_SKILLS`) and not by
/// size. Skill bodies are operator-authored free text with no length limit, so a
/// few long skills could occupy more of the prompt than the conversation they
/// exist to steer — and nothing fails, the model just has less room and answers
/// worse.
///
/// # Why this is duplicated rather than shared
///
/// execution-core's governed loop has the same budget in
/// `runtime_loop::skill_budget`. The two loops are separate crates with separate
/// deployment cadence and neither depends on the other — the same reasoning as
/// the tool-retry vocabularies (`CLAUDE.md`;
/// `docs/postmortem/0001-harn-1-2-tool-dispatch-unification.md`). A contract
/// test (`tests/skill_budget_contract.rs`) pins the two budgets together, so a
/// change on one side that is not made on the other fails the build rather than
/// letting chat and deployed agents disagree about how much prompt skills may
/// take.
pub const SKILL_CONTEXT_BUDGET_CHARS: usize = 8_000;

/// Least content a truncated skill block may keep and still be worth injecting.
pub const MIN_SKILL_CHARS: usize = 240;

/// Appended to a truncated block so the model knows the rule it is reading is
/// incomplete instead of acting on half of it as if it were whole.
/// Render a skill recovery as the tool output the model reads.
///
/// Shared by both loops through `mp_contracts::skill_recovery`; only the
/// rendering lives here, for the same reason the block layout does — what must
/// agree between the loops is the selection rule, not the wording.
///
/// Every arm is a distinct fact and says which it is: a disabled rule presented
/// as a live one is a rule the operator switched off still steering answers, and
/// a miss that suggests nothing is a dead end where a correction was available.
#[must_use]
pub fn render_recovered_skill(recovered: &mp_contracts::skill_recovery::RecoveredSkill) -> String {
    use mp_contracts::skill_recovery::RecoveredSkill;

    match recovered {
        RecoveredSkill::Found {
            name,
            description,
            content,
            truncated,
        } => serde_json::json!({
            "status": "ok",
            "name": name,
            "description": description,
            "instruction": content,
            // Stated even here: this module exists because a silently cut
            // instruction reads as a complete one.
            "truncated": truncated,
        })
        .to_string(),
        RecoveredSkill::Disabled { name } => serde_json::json!({
            "status": "disabled",
            "name": name,
            "detail": "this skill is switched off for the organization; do not follow it, and do \
                       not describe it as a current rule",
        })
        .to_string(),
        RecoveredSkill::NotFound { available } => serde_json::json!({
            "status": "not_found",
            "available": available,
            "detail": "no skill by that name. Use one of the names listed, exactly as written, or \
                       proceed without it — do not guess at the missing instruction",
        })
        .to_string(),
    }
}

pub const TRUNCATION_MARKER: &str =
    "\n[… skill truncated to fit the context budget. Call reattach_skill with this \
     skill's name to read the rest before acting on it.]";

/// Outcome of fitting skill blocks into the budget.
pub struct FittedSkills {
    pub blocks: Vec<String>,
    pub truncated: usize,
    pub dropped: usize,
}

/// Fit already-formatted skill blocks into [`SKILL_CONTEXT_BUDGET_CHARS`],
/// truncating before dropping. Mirrors execution-core's
/// `runtime_loop::skill_budget::fit_skill_blocks`.
///
/// Callers pass blocks already ordered by relevance: the budget is spent front
/// to back, so ordering decides what survives.
#[must_use]
pub fn fit_skill_blocks(blocks: Vec<String>) -> FittedSkills {
    let mut kept = Vec::new();
    let mut truncated = 0usize;
    let mut dropped = 0usize;
    let mut remaining = SKILL_CONTEXT_BUDGET_CHARS;
    let marker_len = TRUNCATION_MARKER.chars().count();

    for block in blocks {
        let len = block.chars().count();
        if len <= remaining {
            remaining -= len;
            kept.push(block);
            continue;
        }
        if remaining <= marker_len + MIN_SKILL_CHARS {
            dropped += 1;
            continue;
        }
        let keep = remaining - marker_len;
        let head: String = block.chars().take(keep).collect();
        remaining = 0;
        truncated += 1;
        kept.push(format!("{head}{TRUNCATION_MARKER}"));
    }

    FittedSkills {
        blocks: kept,
        truncated,
        dropped,
    }
}

#[cfg(test)]
mod skill_budget_tests {
    use super::{fit_skill_blocks, MIN_SKILL_CHARS, SKILL_CONTEXT_BUDGET_CHARS, TRUNCATION_MARKER};

    fn block(name: &str, len: usize) -> String {
        format!("## Skill: {name}\n{}", "x".repeat(len))
    }

    #[test]
    fn the_budget_is_never_exceeded() {
        for sizes in [vec![10_000], vec![5_000; 3], vec![100; 200]] {
            let blocks = sizes
                .iter()
                .enumerate()
                .map(|(i, len)| block(&format!("S{i}"), *len))
                .collect();
            let fitted = fit_skill_blocks(blocks);
            let total: usize = fitted.blocks.iter().map(|b| b.chars().count()).sum();
            assert!(
                total <= SKILL_CONTEXT_BUDGET_CHARS,
                "injected {total} chars"
            );
        }
    }

    #[test]
    fn an_oversized_block_is_truncated_and_says_so() {
        let fitted = fit_skill_blocks(vec![block("Big", SKILL_CONTEXT_BUDGET_CHARS * 2)]);
        assert_eq!(fitted.truncated, 1);
        assert_eq!(fitted.dropped, 0);
        assert!(fitted.blocks[0].ends_with(TRUNCATION_MARKER));
    }

    #[test]
    fn blocks_with_no_useful_room_are_dropped_and_counted() {
        let fitted = fit_skill_blocks(vec![
            block("Fills", SKILL_CONTEXT_BUDGET_CHARS),
            block("NoRoom", MIN_SKILL_CHARS * 4),
        ]);
        assert_eq!(fitted.dropped, 1, "omissions must be counted, never silent");
    }
}
