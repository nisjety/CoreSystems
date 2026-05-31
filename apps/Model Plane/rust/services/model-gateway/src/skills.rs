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
    GetSkillRequest, GetSkillResponse, ListSkillsRequest, ListSkillsResponse, MatchSkillsRequest,
    MatchSkillsResponse, Skill, SkillMatch,
};

const DEFAULT_MATCH_LIMIT: i32 = 5;
const MAX_MATCH_LIMIT: i32 = 20;

#[derive(Clone, Default, Debug)]
pub struct SkillStore {
    // Keyed by (org_id, skill_id) so multi-tenant skill sets don't
    // bleed. Cheap clone (Arc<DashMap>).
    inner: Arc<DashMap<(String, String), Skill>>,
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

/// Lists all skills registered for `req.org_id`.
///
/// # Errors
///
/// Returns `Status::invalid_argument` if `req.org_id` is empty.
pub fn handle_list_skills(
    store: &SkillStore,
    req: ListSkillsRequest,
) -> Result<ListSkillsResponse, Status> {
    if req.org_id.is_empty() {
        return Err(Status::invalid_argument("org_id is required"));
    }
    Ok(ListSkillsResponse {
        request_id: req.request_id,
        skills: store.list(&req.org_id),
    })
}

/// Fetches a single skill by id.
///
/// # Errors
///
/// Returns `Status::not_found` if no skill matches `(org_id, skill_id)`.
pub fn handle_get_skill(
    store: &SkillStore,
    req: GetSkillRequest,
) -> Result<GetSkillResponse, Status> {
    let s = store
        .get(&req.org_id, &req.skill_id)
        .ok_or_else(|| Status::not_found(format!("skill not found: {}", req.skill_id)))?;
    Ok(GetSkillResponse {
        request_id: req.request_id,
        skill: Some(s),
    })
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

/// Ranks an org's skills by keyword overlap with the query.
///
/// # Errors
///
/// Returns `Status::invalid_argument` if `req.org_id` is empty.
pub fn handle_match_skills(
    store: &SkillStore,
    req: MatchSkillsRequest,
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
        let resp = handle_match_skills(
            &store,
            MatchSkillsRequest {
                request_id: "t".into(),
                org_id: "o".into(),
                query: "how do I run pytest".into(),
                limit: 0,
                min_score: 0.0,
            },
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
}
