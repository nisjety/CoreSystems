//! Wave 10d — skills loader.
//!
//! v2 read .md skill files from disk (`auto_registry` + bundled). v1
//! keeps the same shape but the source-of-truth is an in-memory
//! registry — operators preload it at startup from disk or push
//! updates via gRPC. Skill bodies are markdown text shown to the
//! agent as system context when `MatchSkills` surfaces a relevant skill.

// tonic::Status is the unavoidable large Err for gRPC handlers; boxing breaks the service-trait contract.
#![allow(clippy::result_large_err)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError};

use dashmap::DashMap;
use mp_ids::new_ulid;
use tonic::Status;

use mp_contracts::model_plane::v1::{
    AgentSkill, GetSkillRequest, GetSkillResponse, ListSkillsRequest, ListSkillsResponse,
    MatchSkillsRequest, MatchSkillsResponse, Skill, SkillMatch,
};

// The first-party Office skills live in their own file (WS19-S9). lib.rs does not
// declare it yet, so it is declared here; drop this attribute when lib.rs does.
#[path = "builtin_skills.rs"]
pub mod builtin_skills;

const DEFAULT_MATCH_LIMIT: i32 = 5;
const MAX_MATCH_LIMIT: i32 = 20;

/// Provenance prefix stamped on skills pulled from session-core, so a refresh
/// can replace exactly those without disturbing disk/operator-pushed skills.
/// Single source of truth for the places that need to agree. Org-wide skills
/// carry `session-core:org:<origin>` and a user's own or shared ones
/// `session-core:user:<owner>:<origin>`, so a cached entry says whose it is.
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

/// Which pull a learned skill came in with. A pull is made with one caller's
/// bearer, and session-core answers it with the org's skills plus that caller's
/// own and shared ones; the two halves are cached apart so one caller's pull can
/// never replace, or wipe, what another caller's pull brought.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Bucket {
    Org,
    User(String),
}

/// Which buckets hold which learned skills. A skill can be held by two (a skill
/// its owner shared appears in the owner's pull and in the grantee's), and it
/// leaves the cache only when the last bucket holding it lets go.
#[derive(Default, Debug)]
struct BucketBook {
    // (org_id, bucket) -> the skill ids that bucket's latest pull held.
    members: BTreeMap<(String, Bucket), BTreeSet<String>>,
    // (org_id, skill_id) -> the buckets holding it.
    holders: BTreeMap<(String, String), BTreeSet<Bucket>>,
}

impl BucketBook {
    /// Make `bucket`'s members exactly `fresh`, and return the skills no bucket
    /// holds any more.
    fn replace(&mut self, org_id: &str, bucket: &Bucket, fresh: BTreeSet<String>) -> Vec<String> {
        for id in &fresh {
            self.holders
                .entry((org_id.to_owned(), id.clone()))
                .or_default()
                .insert(bucket.clone());
        }
        let previous = self
            .members
            .insert((org_id.to_owned(), bucket.clone()), fresh.clone())
            .unwrap_or_default();
        let mut orphaned = Vec::new();
        for id in previous.difference(&fresh) {
            let key = (org_id.to_owned(), id.clone());
            let Some(holding) = self.holders.get_mut(&key) else {
                continue;
            };
            holding.remove(bucket);
            if holding.is_empty() {
                self.holders.remove(&key);
                orphaned.push(id.clone());
            }
        }
        orphaned
    }
}

#[derive(Clone, Default, Debug)]
pub struct SkillStore {
    // Keyed by (org_id, skill_id) so multi-tenant skill sets don't
    // bleed. Cheap clone (Arc<DashMap>).
    inner: Arc<DashMap<(String, String), Skill>>,
    // When each (org, caller)'s LEARNED skills (session-core agent_skills) were
    // last pulled via the §G7 lazy-load, so the pull is re-done on a TTL rather
    // than exactly once per process lifetime, and per caller because a pull
    // only brings what that caller may see.
    loaded: Arc<DashMap<(String, String), std::time::Instant>>,
    // Bookkeeping for the learned skills in `inner`; taken for the length of one
    // refresh, never across an await.
    book: Arc<Mutex<BucketBook>>,
    // Whether the first-party Office skills are listed for every org (WS19-S9). Read
    // once, when the store is built, so a running process never changes its answers.
    builtin_office: bool,
}

impl SkillStore {
    #[must_use]
    pub fn new() -> Self {
        Self {
            builtin_office: builtin_skills::enabled(),
            ..Self::default()
        }
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

    /// Whether `caller_user_id`'s pull of this org's learned skills from
    /// session-core (G7 read path) is recent enough to reuse. Goes stale on a TTL
    /// so an operator's edit or deletion takes effect without a gateway restart.
    #[must_use]
    pub fn is_loaded(&self, org_id: &str, caller_user_id: &str) -> bool {
        self.loaded
            .get(&(org_id.to_owned(), caller_user_id.to_owned()))
            .is_some_and(|at| at.elapsed() < skill_cache_ttl())
    }

    /// Make the cache match a fresh session-core pull made for `caller_user_id`,
    /// then mark that pull loaded.
    ///
    /// Only skills their owner has accepted are cached at all: a proposal, or a
    /// rejected skill, never reaches a matcher. The pull splits into the org's
    /// skills and the caller's own and shared ones, and each half replaces only
    /// its own bucket: another caller's skills, and the org's skills a different
    /// caller brought, are left as they were. Ownership (SKILL-1) is recorded
    /// from the same pull, and a learned skill with no ownership record is not
    /// usable by anyone.
    ///
    /// Upserts first and prunes second, deliberately: evicting up front would
    /// leave a window where a concurrent turn matches against an org with no
    /// learned skills at all. Pruning is also what makes DELETION take effect —
    /// re-upserting alone would leave a removed skill steering answers forever —
    /// and a skill held by two buckets survives until both let go. Only
    /// `session-core:`-sourced entries are ever pruned, so disk/operator-pushed
    /// skills survive a refresh.
    pub fn refresh_learned(
        &self,
        ownership: &crate::ownership::OwnershipStore,
        org_id: &str,
        caller_user_id: &str,
        pulled: Vec<AgentSkill>,
    ) {
        let mut org_ids = BTreeSet::new();
        let mut user_ids = BTreeSet::new();
        for agent in pulled.into_iter().filter(owner_accepted) {
            let ids = match crate::ownership::Scope::from_wire(&agent.scope) {
                crate::ownership::Scope::Org => &mut org_ids,
                crate::ownership::Scope::User => &mut user_ids,
            };
            ids.insert(agent.id.clone());
            ownership.set(
                org_id,
                crate::ownership::KIND_SKILL,
                &agent.id,
                skill_ownership(&agent),
            );
            self.upsert(org_id, agent_skill_to_skill(agent));
        }
        let orphaned: Vec<String> = {
            let mut book = self.book.lock().unwrap_or_else(PoisonError::into_inner);
            let mut orphaned = book.replace(org_id, &Bucket::Org, org_ids);
            orphaned.extend(book.replace(
                org_id,
                &Bucket::User(caller_user_id.to_owned()),
                user_ids,
            ));
            orphaned
        };
        for id in orphaned {
            self.inner.remove(&(org_id.to_owned(), id.clone()));
            ownership.remove(org_id, crate::ownership::KIND_SKILL, &id);
        }
        self.loaded.insert(
            (org_id.to_owned(), caller_user_id.to_owned()),
            std::time::Instant::now(),
        );
    }

    fn list(&self, org_id: &str) -> Vec<Skill> {
        let mut out: Vec<Skill> = self
            .inner
            .iter()
            .filter(|e| e.key().0 == org_id)
            .map(|e| e.value().clone())
            .collect();
        if self.builtin_office {
            let builtins: Vec<Skill> = builtin_skills::skills()
                .into_iter()
                .filter(|builtin| !out.iter().any(|stored| stored.id == builtin.id))
                .collect();
            out.extend(builtins);
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    fn get(&self, org_id: &str, skill_id: &str) -> Option<Skill> {
        let stored = self
            .inner
            .get(&(org_id.to_string(), skill_id.to_string()))
            .map(|s| s.value().clone());
        stored.or_else(|| {
            if self.builtin_office {
                builtin_skills::skill(skill_id)
            } else {
                None
            }
        })
    }
}

/// Whether `caller_user_id` may use this cached skill. A disk or
/// operator-pushed skill has no ownership record and stays usable by everyone;
/// a learned one (source `session-core:`) is usable only with an ownership
/// record that allows the caller. Its absence fails closed: a learned skill
/// whose owner the cache does not know is nobody's.
fn skill_usable(
    ownership: &crate::ownership::OwnershipStore,
    org_id: &str,
    skill: &Skill,
    caller_user_id: &str,
) -> bool {
    if skill.source_path.starts_with(LEARNED_SOURCE_PREFIX) {
        ownership.usable(
            org_id,
            crate::ownership::KIND_SKILL,
            &skill.id,
            caller_user_id,
        )
    } else {
        ownership.usable_or_untracked(
            org_id,
            crate::ownership::KIND_SKILL,
            &skill.id,
            caller_user_id,
        )
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
        .filter(|s| skill_usable(ownership, &req.org_id, s, caller_user_id))
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
    if !skill_usable(ownership, &req.org_id, &s, caller_user_id) {
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
    let source_path = match crate::ownership::Scope::from_wire(&a.scope) {
        crate::ownership::Scope::Org => format!("{LEARNED_SOURCE_PREFIX}org:{}", a.origin),
        crate::ownership::Scope::User => {
            format!(
                "{LEARNED_SOURCE_PREFIX}user:{}:{}",
                a.owner_user_id, a.origin
            )
        }
    };
    Skill {
        id: a.id,
        name: a.name,
        body: a.content,
        tags: a.trigger_keywords,
        source_path,
        min_score: 0.0,
    }
}

/// Whether a session-core skill may reach a matcher: its owner has accepted it.
/// A row from a session-core that predates review carries no state and is an
/// ordinary accepted skill.
#[must_use]
pub fn owner_accepted(a: &AgentSkill) -> bool {
    a.review_state.is_empty() || a.review_state == "accepted"
}

/// Derive an [`Ownership`](crate::ownership::Ownership) record from a
/// session-core [`AgentSkill`]'s `scope`/`owner_user_id`/`shared_with`
/// fields (SKILL-1).
fn skill_ownership(a: &AgentSkill) -> crate::ownership::Ownership {
    match crate::ownership::Scope::from_wire(&a.scope) {
        crate::ownership::Scope::Org => crate::ownership::Ownership::org(),
        crate::ownership::Scope::User => crate::ownership::Ownership {
            scope: crate::ownership::Scope::User,
            owner_user_id: a.owner_user_id.clone(),
            shared_with: a.shared_with.clone(),
        },
    }
}

/// Format a matched skill as a system-context block for live prompt injection.
/// This is the shape the SSE chat path prepends so a triggered skill steers the
/// model (Claude-Code skill semantics).
/// Skills the member named explicitly for this turn — the composer's `/`
/// picker — in the order named, capped at `max`.
///
/// Explicit selection is the one way a skill reaches a turn WITHOUT keyword
/// overlap: a member who picked "Innkjøpsrutine" gets it whether or not their
/// message happens to contain its trigger words, and `min_score` does not
/// apply. Two things are still never trusted from the client:
///
/// - the id must exist in this org's catalogue — a stale or foreign id is
///   dropped, with a warning, rather than failing the turn over a label;
/// - the caller must be allowed to use it, by the same SKILL-1 ownership rule
///   [`handle_match_skills`] applies (`usable_or_untracked`), so a private
///   skill cannot be pulled into a turn by guessing its id.
///
/// Duplicates collapse to the first mention. Empty bodies are kept here and
/// dropped where blocks are formatted, exactly as matched skills are.
#[must_use]
pub fn resolve_requested_skills(
    store: &SkillStore,
    org_id: &str,
    ownership: &crate::ownership::OwnershipStore,
    caller_user_id: &str,
    requested: &[String],
    max: usize,
) -> Vec<Skill> {
    let mut out: Vec<Skill> = Vec::new();
    for raw in requested {
        if out.len() >= max {
            tracing::warn!(%org_id, max, "explicit skill selection exceeded its cap; extra ids ignored");
            break;
        }
        let id = raw.trim();
        if id.is_empty() || out.iter().any(|s| s.id == id) {
            continue;
        }
        let Some(skill) = store.get(org_id, id) else {
            tracing::warn!(%org_id, skill_id = %id, "requested skill is not in this org's catalogue; ignored");
            continue;
        };
        if !skill_usable(ownership, org_id, &skill, caller_user_id) {
            tracing::warn!(%org_id, skill_id = %id, "requested skill is not usable by the caller; ignored");
            continue;
        }
        out.push(skill);
    }
    out
}

/// The skill a `skill_file` call names, resolved the way the injection path
/// resolves a skill (WS19-S7): this org's catalogue, under the same visibility
/// rule. A skill the turn could not have been given cannot have its bundle read.
/// Matched like `reattach_skill`: case-insensitive and whitespace-trimmed, first
/// usable match.
#[must_use]
pub fn usable_skill_named(
    store: &SkillStore,
    org_id: &str,
    ownership: &crate::ownership::OwnershipStore,
    caller_user_id: &str,
    name: &str,
) -> Option<Skill> {
    let wanted = name.trim().to_lowercase();
    store.list(org_id).into_iter().find(|skill| {
        skill.name.trim().to_lowercase() == wanted
            && skill_usable(ownership, org_id, skill, caller_user_id)
    })
}

#[must_use]
pub fn format_skill_block(s: &Skill) -> String {
    format!("## Skill: {}\n{}", s.name, s.body)
}

/// The footer that tells the model a skill's bundle exists and how to read it
/// (WS19-S7). The bundle's files stay out of the prompt: the model asks for the
/// ones it needs, through `skill_file`.
#[must_use]
pub fn bundle_footer(name: &str, file_count: i32, total_bytes: i64) -> String {
    format!(
        "\n[Bundle: {file_count} files, {total_bytes} bytes. Read them with skill_file, naming \"{name}\": without a path it lists the files; with a path it reads that file.]"
    )
}

/// Keyword overlap score. Lowercases everything and counts how many
/// of the query's tokens appear in the skill's name/tags/body. Cheap
/// and good enough for the < 100 skills regime; promote to BM25 if
/// catalogue grows.
fn score(query_terms: &[String], s: &Skill) -> f32 {
    if query_terms.is_empty() {
        return 0.0;
    }
    // A first-party skill is found by its name and trigger keywords alone. Its body is
    // instruction prose that shares words with almost every request, and matching on it
    // would put all four Office skills into every answer's budget.
    let haystack = if builtin_skills::is_builtin(s) {
        format!("{} {}", s.name, s.tags.join(" "))
    } else {
        format!("{} {} {}", s.name, s.tags.join(" "), s.body)
    }
    .to_lowercase();
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
/// A user's own skill and an org skill may share a name (the store's key allows
/// it). The caller's own one wins: the other is left out of their results, so
/// the model is never given two versions of one rule.
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

    let usable: Vec<Skill> = store
        .list(&req.org_id)
        .into_iter()
        .filter(|s| skill_usable(ownership, &req.org_id, s, caller_user_id))
        .collect();
    let owns = |s: &Skill| {
        ownership
            .get(&req.org_id, crate::ownership::KIND_SKILL, &s.id)
            .is_some_and(|o| {
                o.scope == crate::ownership::Scope::User && o.owner_user_id == caller_user_id
            })
    };
    let owned_names: BTreeSet<String> = usable
        .iter()
        .filter(|s| owns(s))
        .map(|s| s.name.trim().to_lowercase())
        .collect();
    let mut scored: Vec<SkillMatch> = usable
        .into_iter()
        .filter(|s| owns(s) || !owned_names.contains(&s.name.trim().to_lowercase()))
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
        let ownership = crate::ownership::OwnershipStore::new();
        assert!(!store.is_loaded("o", "alice"));
        store.refresh_learned(&ownership, "o", "alice", vec![]);
        assert!(store.is_loaded("o", "alice"));
        assert!(!store.is_loaded("other", "alice"), "tracking is per-org");
        assert!(
            !store.is_loaded("o", "bob"),
            "a pull only brings what its caller may see, so each caller pulls for themselves"
        );
    }

    /// A skill as session-core lists it: accepted, org-wide unless `owner` is
    /// given, in which case it is that user's own.
    fn agent(id: &str, name: &str, owner: &str) -> AgentSkill {
        AgentSkill {
            id: id.into(),
            name: name.into(),
            content: format!("body of {name}"),
            enabled: true,
            origin: "background_review".into(),
            scope: if owner.is_empty() { "org" } else { "user" }.into(),
            owner_user_id: owner.into(),
            review_state: "accepted".into(),
            ..Default::default()
        }
    }

    fn names(store: &SkillStore, org: &str) -> Vec<String> {
        store.list(org).into_iter().map(|s| s.name).collect()
    }

    fn matches_for(
        store: &SkillStore,
        ownership: &crate::ownership::OwnershipStore,
        caller: &str,
        query: &str,
    ) -> Vec<String> {
        handle_match_skills(
            store,
            MatchSkillsRequest {
                request_id: "t".into(),
                org_id: "o".into(),
                query: query.into(),
                limit: 0,
                min_score: 0.0,
            },
            ownership,
            caller,
        )
        .unwrap()
        .matches
        .into_iter()
        .filter_map(|m| m.skill.map(|s| s.id))
        .collect()
    }

    #[test]
    fn stale_org_is_repulled_so_operator_edits_land_without_a_restart() {
        // The regression: `loaded` was a permanent marker, so an org was pulled
        // exactly once per process and later edits were invisible until restart.
        let store = SkillStore::new();
        store.loaded.insert(
            ("o".to_owned(), "alice".to_owned()),
            std::time::Instant::now() - skill_cache_ttl(),
        );

        assert!(
            !store.is_loaded("o", "alice"),
            "a cache entry older than the TTL must read as stale"
        );
    }

    #[test]
    fn refresh_learned_reflects_an_edited_skill_body() {
        let store = SkillStore::new();
        let ownership = crate::ownership::OwnershipStore::new();
        store.refresh_learned(
            &ownership,
            "o",
            "alice",
            vec![agent("s1", "invoice-policy", "")],
        );
        store.refresh_learned(
            &ownership,
            "o",
            "alice",
            vec![AgentSkill {
                content: "EDITED body".into(),
                ..agent("s1", "invoice-policy", "")
            }],
        );

        let listed = store.list("o");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].body, "EDITED body");
    }

    #[test]
    fn refresh_learned_forgets_a_deleted_skill() {
        // Re-upserting alone would leave a deleted skill steering answers forever.
        let store = SkillStore::new();
        let ownership = crate::ownership::OwnershipStore::new();
        store.refresh_learned(
            &ownership,
            "o",
            "alice",
            vec![agent("s1", "keep-me", ""), agent("s2", "delete-me", "")],
        );
        store.refresh_learned(&ownership, "o", "alice", vec![agent("s1", "keep-me", "")]);

        assert_eq!(
            names(&store, "o"),
            vec!["keep-me".to_owned()],
            "deleted skill lingered"
        );
        assert!(
            ownership
                .get("o", crate::ownership::KIND_SKILL, "s2")
                .is_none(),
            "a forgotten skill keeps no ownership record"
        );
    }

    #[test]
    fn refresh_learned_leaves_operator_pushed_skills_alone() {
        // Disk/operator-pushed skills have no session-core provenance and must
        // survive a learned-skill refresh.
        let store = SkillStore::new();
        let ownership = crate::ownership::OwnershipStore::new();
        let pushed = store.upsert("o", s("operator-skill", &["ops"], "pushed body"));
        store.refresh_learned(
            &ownership,
            "o",
            "alice",
            vec![agent("s1", "learned-skill", "")],
        );
        store.refresh_learned(&ownership, "o", "alice", vec![]);

        assert!(
            names(&store, "o").contains(&"operator-skill".to_owned()),
            "refresh clobbered a non-learned skill: {:?}",
            names(&store, "o")
        );
        assert!(store.get("o", &pushed.id).is_some());
    }

    #[test]
    fn refresh_learned_does_not_touch_another_org() {
        let store = SkillStore::new();
        let ownership = crate::ownership::OwnershipStore::new();
        store.refresh_learned(&ownership, "o", "alice", vec![agent("s1", "mine", "")]);
        store.refresh_learned(
            &ownership,
            "other",
            "alice",
            vec![agent("s9", "theirs", "")],
        );
        store.refresh_learned(&ownership, "other", "alice", vec![]);

        assert_eq!(
            store.list("o").len(),
            1,
            "sibling org's refresh bled across"
        );
    }

    #[test]
    fn user_scope_skill_does_not_flap_between_callers() {
        // The regression: every pull replaced the whole org's learned skills with
        // what the triggering caller could see, so alice's private skill vanished
        // from the cache the moment bob's pull landed, and came back only when
        // alice's own cache entry expired.
        let store = SkillStore::new();
        let ownership = crate::ownership::OwnershipStore::new();
        let org_wide = agent("org-1", "house style", "");
        store.refresh_learned(
            &ownership,
            "o",
            "alice",
            vec![org_wide.clone(), agent("alice-1", "alice runbook", "alice")],
        );
        store.refresh_learned(&ownership, "o", "bob", vec![org_wide.clone()]);

        assert!(
            store.is_loaded("o", "alice"),
            "bob's pull did not expire alice's"
        );
        assert_eq!(
            matches_for(&store, &ownership, "alice", "alice runbook house style"),
            vec!["alice-1".to_owned(), "org-1".to_owned()],
            "alice still has her skill after bob's pull"
        );
        assert_eq!(
            matches_for(&store, &ownership, "bob", "alice runbook house style"),
            vec!["org-1".to_owned()],
            "bob never sees alice's skill"
        );

        // And alice's pull in turn leaves what bob's brought alone.
        store.refresh_learned(
            &ownership,
            "o",
            "bob",
            vec![org_wide, agent("bob-1", "bob notes", "bob")],
        );
        store.refresh_learned(
            &ownership,
            "o",
            "alice",
            vec![agent("org-1", "house style", "")],
        );
        assert_eq!(
            matches_for(&store, &ownership, "bob", "bob notes"),
            vec!["bob-1".to_owned()],
            "alice's refresh wiped bob's skill"
        );
    }

    #[test]
    fn learned_skill_without_ownership_is_not_usable() {
        // A learned skill the cache cannot say whose it is belongs to no one:
        // the untracked rule that keeps disk skills visible must not cover it.
        let store = SkillStore::new();
        let ownership = crate::ownership::OwnershipStore::new();
        let orphan = store.upsert(
            "o",
            Skill {
                source_path: format!("{LEARNED_SOURCE_PREFIX}org:background_review"),
                ..s("orphan", &["orphan"], "orphan body")
            },
        );
        let disk = store.upsert("o", s("disk", &["orphan"], "disk body"));

        assert_eq!(
            matches_for(&store, &ownership, "alice", "orphan"),
            vec![disk.id.clone()],
            "only the disk skill matches"
        );
        let listed = handle_list_skills(
            &store,
            ListSkillsRequest {
                request_id: "t".into(),
                org_id: "o".into(),
            },
            &ownership,
            "alice",
        )
        .unwrap();
        assert_eq!(listed.skills.len(), 1);
        assert!(handle_get_skill(
            &store,
            GetSkillRequest {
                request_id: "t".into(),
                org_id: "o".into(),
                skill_id: orphan.id.clone(),
            },
            &ownership,
            "alice",
        )
        .is_err());
        assert!(
            resolve_requested_skills(&store, "o", &ownership, "alice", &[orphan.id], 4).is_empty(),
            "an explicit pick cannot pull it in either"
        );
    }

    #[test]
    fn shared_skill_survives_other_bucket_prune() {
        // A skill its owner shared sits in the owner's bucket and in the
        // grantee's. Either bucket letting go of it is not the skill being gone.
        let store = SkillStore::new();
        let ownership = crate::ownership::OwnershipStore::new();
        let shared = AgentSkill {
            shared_with: vec!["bob".into()],
            ..agent("shared-1", "shared notes", "alice")
        };
        store.refresh_learned(&ownership, "o", "alice", vec![shared.clone()]);
        store.refresh_learned(&ownership, "o", "bob", vec![shared]);

        // alice stops listing it (she unshared it, or deleted it); bob's cache
        // entry still holds it until his own pull says otherwise.
        store.refresh_learned(&ownership, "o", "alice", vec![]);
        assert_eq!(
            matches_for(&store, &ownership, "bob", "shared notes"),
            vec!["shared-1".to_owned()]
        );

        store.refresh_learned(&ownership, "o", "bob", vec![]);
        assert!(store.list("o").is_empty(), "the last holder let go");
        assert!(ownership
            .get("o", crate::ownership::KIND_SKILL, "shared-1")
            .is_none());
    }

    #[test]
    fn owner_skill_wins_name_tie() {
        // The unique key is (org, scope, owner, name): an org skill and alice's
        // own may share a name. Alice gets hers; everyone else gets the org's.
        let store = SkillStore::new();
        let ownership = crate::ownership::OwnershipStore::new();
        let org_wide = agent("org-deploy", "deploy", "");
        let alices = agent("alice-deploy", "deploy", "alice");
        store.refresh_learned(&ownership, "o", "alice", vec![org_wide.clone(), alices]);
        store.refresh_learned(&ownership, "o", "bob", vec![org_wide]);

        assert_eq!(
            matches_for(&store, &ownership, "alice", "deploy"),
            vec!["alice-deploy".to_owned()],
            "alice's own skill wins the tie and the org's is left out"
        );
        assert_eq!(
            matches_for(&store, &ownership, "bob", "deploy"),
            vec!["org-deploy".to_owned()]
        );
    }

    #[test]
    fn proposals_and_rejected_skills_never_reach_the_cache() {
        let store = SkillStore::new();
        let ownership = crate::ownership::OwnershipStore::new();
        let proposal = AgentSkill {
            review_state: "proposed".into(),
            ..agent("p-1", "proposal", "alice")
        };
        let rejected = AgentSkill {
            review_state: "rejected".into(),
            ..agent("r-1", "rejected", "alice")
        };
        // An older session-core sends no state: an ordinary accepted skill.
        let legacy = AgentSkill {
            review_state: String::new(),
            ..agent("l-1", "legacy", "")
        };
        store.refresh_learned(&ownership, "o", "alice", vec![proposal, rejected, legacy]);

        assert_eq!(names(&store, "o"), vec!["legacy".to_owned()]);
        assert!(ownership
            .get("o", crate::ownership::KIND_SKILL, "p-1")
            .is_none());
    }

    #[test]
    fn learned_source_path_says_whose_skill_it_is() {
        assert_eq!(
            agent_skill_to_skill(agent("s1", "a", "")).source_path,
            "session-core:org:background_review"
        );
        assert_eq!(
            agent_skill_to_skill(agent("s2", "b", "alice")).source_path,
            "session-core:user:alice:background_review"
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
            enabled: true,
            origin: "background_review".into(),
            scope: "org".into(),
            review_state: "accepted".into(),
            ..Default::default()
        };
        let sk = agent_skill_to_skill(a.clone());
        assert_eq!(sk.id, "sk-1");
        assert_eq!(sk.name, "Cache Tips");
        assert_eq!(sk.body, "use the cache"); // content -> body
        assert_eq!(sk.tags, vec!["cache".to_owned(), "perf".to_owned()]); // keywords -> tags
        assert_eq!(sk.source_path, "session-core:org:background_review"); // provenance marker
                                                                          // A learned skill, once refreshed in, is matchable by its keywords.
        let store = SkillStore::new();
        let ownership = crate::ownership::OwnershipStore::new();
        store.refresh_learned(&ownership, "o", "caller", vec![a]);
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
            "learned skill should be matchable after mapping+refresh"
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

/// What `skill_file` says when it cannot read a bundle: no such skill for this
/// caller, no bundle, or no file at the path asked for. Each is a fact, and says
/// which one it is.
#[must_use]
pub fn render_bundle_status(status: &str, skill: &str, detail: &str) -> String {
    serde_json::json!({
        "status": status,
        "skill": skill,
        "detail": detail,
    })
    .to_string()
}

/// The files a bundle holds, for a read that names no path. Packed into one
/// read's cap, so a bundle of the largest allowed shape still answers in one
/// result; `truncated` says when the listing could not hold every file.
#[must_use]
pub fn render_bundle_listing(
    skill: &str,
    files: &[mp_contracts::model_plane::v1::SkillBundleFile],
) -> String {
    let mut listed = Vec::new();
    let mut used = 0usize;
    for file in files {
        let entry = serde_json::json!({
            "path": file.path,
            "size_bytes": file.size_bytes,
            "binary": file.binary,
            "executable": file.executable,
        });
        let size = entry.to_string().len();
        if used + size > mp_contracts::skill_bundle::MAX_READ_BYTES {
            break;
        }
        used += size;
        listed.push(entry);
    }
    serde_json::json!({
        "status": "listing",
        "skill": skill,
        "total_files": files.len(),
        "truncated": listed.len() < files.len(),
        "files": listed,
        "detail": "read one file by passing its path",
    })
    .to_string()
}

/// One read of a file (WS19-S7). A read that stops short says where the next one
/// starts, so a long file is reachable in full and a partial read is never taken
/// for a whole one.
#[must_use]
pub fn render_bundle_text(
    skill: &str,
    path: &str,
    offset: usize,
    text: &str,
    next_offset: Option<usize>,
    total_bytes: usize,
) -> String {
    let detail = match next_offset {
        Some(next) => format!(
            "the file goes on; call skill_file again with this path and offset {next} for the rest, before relying on it"
        ),
        None => "end of the file".to_owned(),
    };
    serde_json::json!({
        "status": "ok",
        "skill": skill,
        "path": path,
        "offset": offset,
        "total_bytes": total_bytes,
        "content": text,
        "truncated": next_offset.is_some(),
        "next_offset": next_offset,
        "detail": detail,
    })
    .to_string()
}

/// A file that is not text: its path, size and digest, never its bytes (WS19-S7).
#[must_use]
pub fn render_bundle_binary(
    skill: &str,
    file: &mp_contracts::model_plane::v1::SkillBundleFile,
) -> String {
    serde_json::json!({
        "status": "binary",
        "skill": skill,
        "path": file.path,
        "size_bytes": file.size_bytes,
        "sha256": file.sha256,
        "detail": "this file is not text, so its content is not returned; describe it from its path and size",
    })
    .to_string()
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
    fit_skill_blocks_with_footers(
        blocks
            .into_iter()
            .map(|block| (block, String::new()))
            .collect(),
    )
}

/// [`fit_skill_blocks`] for blocks that end in a footer (WS19-S7). The footer
/// counts against the budget like the rest of its block, but it is never cut: a
/// truncated block keeps its footer after the truncation marker, so the model
/// still learns that the bundle exists. With empty footers the result is exactly
/// what `fit_skill_blocks` returns.
#[must_use]
pub fn fit_skill_blocks_with_footers(blocks: Vec<(String, String)>) -> FittedSkills {
    let mut kept = Vec::new();
    let mut truncated = 0usize;
    let mut dropped = 0usize;
    let mut remaining = SKILL_CONTEXT_BUDGET_CHARS;
    let marker_len = TRUNCATION_MARKER.chars().count();

    for (block, footer) in blocks {
        let footer_len = footer.chars().count();
        let len = block.chars().count() + footer_len;
        if len <= remaining {
            remaining -= len;
            kept.push(format!("{block}{footer}"));
            continue;
        }
        if remaining <= marker_len + footer_len + MIN_SKILL_CHARS {
            dropped += 1;
            continue;
        }
        let keep = remaining - marker_len - footer_len;
        let head: String = block.chars().take(keep).collect();
        remaining = 0;
        truncated += 1;
        kept.push(format!("{head}{TRUNCATION_MARKER}{footer}"));
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

    use super::{resolve_requested_skills, Skill, SkillStore};

    fn owned(id: &str, name: &str, body: &str) -> Skill {
        Skill {
            id: id.into(),
            name: name.into(),
            body: body.into(),
            tags: Vec::new(),
            source_path: String::new(),
            min_score: 0.0,
        }
    }

    #[test]
    fn explicit_selection_needs_no_keyword_overlap_and_keeps_order() {
        let store = SkillStore::new();
        store.upsert("org", owned("sk-b", "Bravo", "b body"));
        store.upsert("org", owned("sk-a", "Alpha", "a body"));
        let ownership = crate::ownership::OwnershipStore::new();
        let got = resolve_requested_skills(
            &store,
            "org",
            &ownership,
            "user-1",
            &["sk-b".to_owned(), "sk-a".to_owned(), "sk-b".to_owned()],
            4,
        );
        let ids: Vec<&str> = got.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["sk-b", "sk-a"],
            "named order, duplicates collapsed"
        );
    }

    #[test]
    fn explicit_selection_drops_unknown_ids_instead_of_failing() {
        let store = SkillStore::new();
        store.upsert("org", owned("sk-a", "Alpha", "a body"));
        let ownership = crate::ownership::OwnershipStore::new();
        let got = resolve_requested_skills(
            &store,
            "org",
            &ownership,
            "user-1",
            &["nope".to_owned(), "sk-a".to_owned()],
            4,
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, "sk-a");
    }

    #[test]
    fn explicit_selection_cannot_pull_in_another_users_private_skill() {
        let store = SkillStore::new();
        store.upsert("org", owned("sk-private", "Private", "p body"));
        store.upsert("org", owned("sk-org", "Org wide", "o body"));
        let ownership = crate::ownership::OwnershipStore::new();
        ownership.replace_org_kind(
            "org",
            crate::ownership::KIND_SKILL,
            vec![
                (
                    "sk-private".to_owned(),
                    crate::ownership::Ownership {
                        scope: crate::ownership::Scope::User,
                        owner_user_id: "someone-else".to_owned(),
                        shared_with: Vec::new(),
                    },
                ),
                ("sk-org".to_owned(), crate::ownership::Ownership::org()),
            ],
        );
        let got = resolve_requested_skills(
            &store,
            "org",
            &ownership,
            "user-1",
            &["sk-private".to_owned(), "sk-org".to_owned()],
            4,
        );
        let ids: Vec<&str> = got.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["sk-org"]);
    }

    #[test]
    fn explicit_selection_is_capped() {
        let store = SkillStore::new();
        for i in 0..6 {
            store.upsert("org", owned(&format!("sk-{i}"), &format!("S{i}"), "body"));
        }
        let ownership = crate::ownership::OwnershipStore::new();
        let requested: Vec<String> = (0..6).map(|i| format!("sk-{i}")).collect();
        let got = resolve_requested_skills(&store, "org", &ownership, "user-1", &requested, 4);
        assert_eq!(got.len(), 4);
    }
}

#[cfg(test)]
mod skill_file_tests {
    use super::{
        bundle_footer, render_bundle_binary, render_bundle_listing, render_bundle_text,
        usable_skill_named, SkillStore,
    };
    use crate::ownership::OwnershipStore;
    use mp_contracts::model_plane::v1::{Skill, SkillBundleFile};

    fn skill(id: &str, name: &str) -> Skill {
        Skill {
            id: id.into(),
            name: name.into(),
            body: format!("Instructions for {name}."),
            tags: Vec::new(),
            source_path: String::new(),
            min_score: 0.0,
        }
    }

    fn file(path: &str, size_bytes: i64, binary: bool) -> SkillBundleFile {
        SkillBundleFile {
            path: path.into(),
            sha256: "ab".repeat(32),
            size_bytes,
            executable: false,
            binary,
        }
    }

    #[test]
    fn skill_file_reads_only_this_orgs_bundle() {
        let store = SkillStore::new();
        store.upsert("org-a", skill("sk-1", "Deploy Runbook"));
        let ownership = OwnershipStore::new();
        assert_eq!(
            usable_skill_named(&store, "org-a", &ownership, "alice", "  deploy runbook ")
                .map(|found| found.id),
            Some("sk-1".to_owned())
        );
        assert!(
            usable_skill_named(&store, "org-b", &ownership, "alice", "Deploy Runbook").is_none(),
            "another org's skill must not resolve, so its bundle is never asked for"
        );
    }

    #[test]
    fn a_bundle_footer_names_the_skill_and_the_way_in() {
        let footer = bundle_footer("Deploy Runbook", 12, 84_000);
        assert!(footer.contains("12 files, 84000 bytes"), "{footer}");
        assert!(
            footer.contains("skill_file, naming \"Deploy Runbook\""),
            "{footer}"
        );
    }

    #[test]
    fn skill_file_refuses_binary_content_by_describing_it() {
        let rendered: serde_json::Value = serde_json::from_str(&render_bundle_binary(
            "Brand",
            &file("templates/logo.png", 2_048, true),
        ))
        .expect("valid JSON");
        assert_eq!(rendered["status"], "binary");
        assert_eq!(rendered["size_bytes"], 2_048);
        assert!(rendered.get("content").is_none(), "{rendered}");
    }

    #[test]
    fn skill_file_pages_at_the_cap_with_a_marker() {
        let partial: serde_json::Value = serde_json::from_str(&render_bundle_text(
            "Deploy Runbook",
            "references/long.md",
            0,
            "first piece",
            Some(32_768),
            80_000,
        ))
        .expect("valid JSON");
        assert_eq!(partial["truncated"], true);
        assert_eq!(partial["next_offset"], 32_768);
        assert!(
            partial["detail"]
                .as_str()
                .unwrap_or_default()
                .contains("offset 32768"),
            "{partial}"
        );

        let whole: serde_json::Value = serde_json::from_str(&render_bundle_text(
            "Deploy Runbook",
            "SKILL.md",
            0,
            "all of it",
            None,
            10,
        ))
        .expect("valid JSON");
        assert_eq!(whole["truncated"], false);
        assert!(whole["next_offset"].is_null());
    }

    #[test]
    fn a_listing_names_every_file_that_fits_one_read() {
        let listing: serde_json::Value = serde_json::from_str(&render_bundle_listing(
            "Deploy Runbook",
            &[
                file("SKILL.md", 120, false),
                file("scripts/run.py", 900, false),
            ],
        ))
        .expect("valid JSON");
        assert_eq!(listing["total_files"], 2);
        assert_eq!(listing["truncated"], false);
        assert_eq!(listing["files"].as_array().map(Vec::len), Some(2));
    }
}
