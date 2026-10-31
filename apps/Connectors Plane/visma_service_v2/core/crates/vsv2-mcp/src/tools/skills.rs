use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::meta::meta;
use crate::skills::{Skill, SkillIndex};
use crate::tool::{parse_args, Tool, ToolDef, ToolError, ToolKind};

pub const LIST: &str = "visma_list_skills";
pub const GET: &str = "visma_get_skill";

const MAX_SEARCH_RESULTS: usize = 8;

/// A text argument counts as given only if it holds something: a model often sends `""` for a field
/// it does not use.
fn present(v: &Option<String>) -> Option<&str> {
    v.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// The tools of a skill that this server instance does not have (a read-only server has no write
/// tools, a server without credentials has no live tools). The skills describe the whole product;
/// this says which steps cannot be followed here.
fn unavailable<'a>(skill: &'a Skill, available: &BTreeSet<String>) -> Vec<&'a str> {
    skill
        .tools
        .iter()
        .filter(|t| !available.contains(*t))
        .map(String::as_str)
        .collect()
}

/// Every tool any skill names: the `available` set of a server that has all of them (tests).
pub fn every_tool_named_by_a_skill(index: &SkillIndex) -> BTreeSet<String> {
    index
        .all()
        .iter()
        .flat_map(|s| s.tools.iter().cloned())
        .collect()
}

// ---------------------------------------------------------------- list

pub struct ListSkills {
    index: Arc<SkillIndex>,
    available: Arc<BTreeSet<String>>,
}

impl ListSkills {
    pub fn new(index: Arc<SkillIndex>, available: Arc<BTreeSet<String>>) -> Self {
        Self { index, available }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListArgs {
    #[serde(default)]
    tag: Option<String>,
    #[serde(default)]
    tool: Option<String>,
    #[serde(default)]
    query: Option<String>,
}

#[async_trait]
impl Tool for ListSkills {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: LIST,
            description: format!(
                "List the bundled usage skills: tested guides that explain how to use this server's tools \
                 accurately (exact table and column facts, conventions, pitfalls, safe order of operations). \
                 Filter by `tool` to get the skills for one tool, by `tag`, or search with `query`. Read a \
                 skill with {GET}. Start with `start-here`."
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "tool": {"type": "string", "description": "a tool name, e.g. warehouse_query"},
                    "tag": {"type": "string", "description": "e.g. finance, writes, live"},
                    "query": {"type": "string", "description": "free text, e.g. overdue invoices aging"}
                },
                "additionalProperties": false
            }),
            kind: ToolKind::Read,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let a: ListArgs = parse_args(args)?;
        let idx = &self.index;
        let (query, tool, tag) = (present(&a.query), present(&a.tool), present(&a.tag));
        let mut rows: Vec<(&Skill, Option<u32>)> = match query {
            Some(q) => idx
                .search(q)
                .into_iter()
                .map(|(s, sc)| (s, Some(sc)))
                .collect(),
            None => idx.all().iter().map(|s| (s, None)).collect(),
        };
        if let Some(t) = tool {
            let ids: Vec<&str> = idx.for_tool(t).iter().map(|s| s.id.as_str()).collect();
            rows.retain(|(s, _)| ids.contains(&s.id.as_str()));
            if query.is_none() {
                // The tool's own order: the skill to read first comes first.
                rows.sort_by_key(|(s, _)| ids.iter().position(|id| *id == s.id.as_str()));
            }
            if rows.is_empty() && !idx.by_tool().contains_key(t) {
                return Err(ToolError::InvalidArgs(format!(
                    "no skills mention a tool named {t:?}; known tools with skills: {}",
                    idx.by_tool().keys().cloned().collect::<Vec<_>>().join(", ")
                )));
            }
        }
        if let Some(tag) = tag {
            rows.retain(|(s, _)| s.tags.iter().any(|g| g.eq_ignore_ascii_case(tag)));
        }
        let skills: Vec<Value> = rows
            .iter()
            .map(|(s, score)| {
                let mut v = s.summary();
                if let Some(sc) = score {
                    v["score"] = json!(sc);
                }
                let missing = unavailable(s, &self.available);
                if !missing.is_empty() {
                    v["unavailable_tools"] = json!(missing);
                }
                v
            })
            .collect();
        Ok(json!({
            "count": skills.len(),
            "available_tags": idx.tags(),
            "skills": skills,
            "meta": meta("bundled_skills", None, json!({"coverage": "all_bundled_skills_matching_the_filter"})),
        }))
    }
}

// ---------------------------------------------------------------- get

pub struct GetSkill {
    index: Arc<SkillIndex>,
    available: Arc<BTreeSet<String>>,
}

impl GetSkill {
    pub fn new(index: Arc<SkillIndex>, available: Arc<BTreeSet<String>>) -> Self {
        Self { index, available }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GetArgs {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    query: Option<String>,
}

#[async_trait]
impl Tool for GetSkill {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: GET,
            description: format!(
                "Read one usage skill in full, by `id` or by a search `query` (best match). Read the skill \
                 for a tool before using that tool for the first time, and always before any write. Skills \
                 contain exact table/column names, conventions and pitfalls plus worked `tool-call` examples. \
                 Find ids with {LIST}."
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "skill id, e.g. warehouse-sql-basics"},
                    "query": {"type": "string", "description": "search text; returns the best match"}
                },
                "additionalProperties": false
            }),
            kind: ToolKind::Read,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let a: GetArgs = parse_args(args)?;
        let idx = &self.index;
        let (skill, also) = match (present(&a.id), present(&a.query)) {
            (Some(id), _) => {
                let s = idx.get(id).ok_or_else(|| {
                    let near: Vec<String> = idx
                        .search(&id.replace('-', " "))
                        .into_iter()
                        .take(3)
                        .map(|(s, _)| s.id.clone())
                        .collect();
                    ToolError::InvalidArgs(format!(
                        "unknown skill {id:?}{}; list them with {LIST}",
                        if near.is_empty() {
                            String::new()
                        } else {
                            format!(" (did you mean: {}?)", near.join(", "))
                        }
                    ))
                })?;
                (s, vec![])
            }
            (None, Some(q)) => {
                let hits = idx.search(q);
                let Some((best, _)) = hits.first().copied() else {
                    return Err(ToolError::Failed(format!(
                        "no skill matches {q:?}; list them with {LIST}"
                    )));
                };
                let also = hits
                    .iter()
                    .skip(1)
                    .take(MAX_SEARCH_RESULTS)
                    .map(|(s, sc)| json!({"id": s.id, "title": s.title, "score": sc}))
                    .collect();
                (best, also)
            }
            (None, None) => {
                return Err(ToolError::InvalidArgs(format!(
                    "give an `id` or a `query` (list skills with {LIST})"
                )))
            }
        };
        let prerequisites: Vec<Value> = skill
            .prerequisites
            .iter()
            .filter_map(|p| idx.get(p))
            .map(|p| json!({"id": p.id, "title": p.title, "description": p.description}))
            .collect();
        let mut out = json!({
            "skill": skill.full(),
            "prerequisites": prerequisites,
            "also_matching": also,
            "meta": meta("bundled_skills", None, json!({"coverage": "single_skill"})),
        });
        let missing = unavailable(skill, &self.available);
        if !missing.is_empty() {
            out["skill"]["unavailable_tools"] = json!(missing);
            out["availability_note"] = json!(format!(
                "This server instance does not have these tools of the skill: {}. {} says why. Skip the steps that need \
                 them and tell the user what could not be done.",
                missing.join(", "),
                crate::tools::init::NAME
            ));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A server that has every tool any skill names.
    fn list(idx: Arc<SkillIndex>) -> ListSkills {
        let all = Arc::new(every_tool_named_by_a_skill(&idx));
        ListSkills::new(idx, all)
    }

    fn get(idx: Arc<SkillIndex>) -> GetSkill {
        let all = Arc::new(every_tool_named_by_a_skill(&idx));
        GetSkill::new(idx, all)
    }

    fn index() -> Arc<SkillIndex> {
        let mk = |id: &str, tags: &str, tools: &str, pre: &str, body: &str| {
            format!("---\nid: {id}\ntitle: Title of {id}\ndescription: Describes {id}\ntags: [{tags}]\ntools: [{tools}]\nprerequisites: [{pre}]\n---\n{body}")
        };
        let (a, b, c) = (
            mk(
                "start-here",
                "meta",
                "visma_init, warehouse_query",
                "",
                "begin here",
            ),
            mk(
                "receivables-aging",
                "finance, ar",
                "warehouse_query",
                "start-here",
                "overdue invoices aging buckets",
            ),
            mk(
                "edit-order",
                "writes",
                "visma_salesorder_patch_plan",
                "start-here",
                "plan then approve",
            ),
        );
        Arc::new(
            SkillIndex::parse(&[
                ("start-here.md", &a),
                ("receivables-aging.md", &b),
                ("edit-order.md", &c),
            ])
            .unwrap(),
        )
    }

    #[tokio::test]
    async fn list_filters_by_tool_tag_and_query_and_reports_the_available_tags() {
        let t = list(index());
        let all = t.call(json!({})).await.unwrap();
        assert_eq!(all["count"], 3);
        assert!(all["available_tags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x == "finance"));
        assert!(
            all["skills"][0].get("content").is_none(),
            "list never returns bodies"
        );

        let by_tool = t.call(json!({"tool": "warehouse_query"})).await.unwrap();
        let ids: Vec<&str> = by_tool["skills"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["receivables-aging", "start-here"]);

        assert_eq!(t.call(json!({"tag": "writes"})).await.unwrap()["count"], 1);
        let q = t.call(json!({"query": "overdue aging"})).await.unwrap();
        assert_eq!(q["skills"][0]["id"], "receivables-aging");
        assert!(q["skills"][0]["score"].as_u64().unwrap() > 0);

        let e = t
            .call(json!({"tool": "nonexistent_tool"}))
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("known tools with skills"), "{e}");
    }

    #[tokio::test]
    async fn list_by_tool_puts_the_primary_skill_first_not_the_alphabetical_one() {
        let mk = |id: &str, primary: &str| {
            format!("---\nid: {id}\ntitle: Title of {id}\ndescription: Describes {id}\ntags: [t]\ntools: [warehouse_query]\nprimary: [{primary}]\n---\nbody of {id}")
        };
        let (alpha, zeta) = (mk("alpha-extra", ""), mk("zeta-basics", "warehouse_query"));
        let idx = Arc::new(
            SkillIndex::parse(&[("alpha-extra.md", &alpha), ("zeta-basics.md", &zeta)]).unwrap(),
        );
        let out = list(idx)
            .call(json!({"tool": "warehouse_query"}))
            .await
            .unwrap();
        assert_eq!(out["skills"][0]["id"], "zeta-basics", "{out}");
        assert_eq!(out["skills"][1]["id"], "alpha-extra");
    }

    #[tokio::test]
    async fn get_by_id_returns_the_body_and_resolved_prerequisites() {
        let t = get(index());
        let out = t.call(json!({"id": "receivables-aging"})).await.unwrap();
        assert_eq!(out["skill"]["content"], "overdue invoices aging buckets");
        assert_eq!(out["prerequisites"][0]["id"], "start-here");
        assert_eq!(out["skill"]["related_tools"][0], "warehouse_query");
    }

    #[tokio::test]
    async fn get_by_query_returns_the_best_match_and_the_others_and_unknown_ids_get_suggestions() {
        let t = get(index());
        let out = t
            .call(json!({"query": "aging of overdue invoices"}))
            .await
            .unwrap();
        assert_eq!(out["skill"]["id"], "receivables-aging");

        let e = t
            .call(json!({"id": "receivables"}))
            .await
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("unknown skill") && e.contains("receivables-aging"),
            "{e}"
        );
        assert!(matches!(
            t.call(json!({})).await,
            Err(ToolError::InvalidArgs(_))
        ));
        assert!(matches!(
            t.call(json!({"query": "zzzzqq"})).await,
            Err(ToolError::Failed(_))
        ));
        assert!(matches!(
            t.call(json!({"id": "x", "bogus": 1})).await,
            Err(ToolError::InvalidArgs(_))
        ));
    }

    #[tokio::test]
    async fn empty_strings_count_as_not_given() {
        let l = list(index());
        let all = l
            .call(json!({"query": "", "tag": "", "tool": ""}))
            .await
            .unwrap();
        assert_eq!(all["count"], 3, "blank filters filter nothing: {all}");
        let blank = l.call(json!({"query": "   "})).await.unwrap();
        assert_eq!(blank["count"], 3);

        let g = get(index());
        let by_query = g
            .call(json!({"id": "", "query": "overdue aging"}))
            .await
            .unwrap();
        assert_eq!(
            by_query["skill"]["id"], "receivables-aging",
            "an empty id falls back to the query"
        );
        let none = g.call(json!({"id": "", "query": ""})).await.unwrap_err();
        assert!(
            none.to_string().contains("give an `id` or a `query`"),
            "{none}"
        );
    }

    #[tokio::test]
    async fn skills_name_the_tools_this_server_does_not_have() {
        let idx = index();
        // a server that only has warehouse_query
        let only_query: Arc<BTreeSet<String>> = Arc::new(["warehouse_query".to_string()].into());
        let l = ListSkills::new(idx.clone(), only_query.clone());
        let out = l.call(json!({})).await.unwrap();
        let by_id = |id: &str| {
            out["skills"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["id"] == id)
                .unwrap()
                .clone()
        };
        assert_eq!(
            by_id("start-here")["unavailable_tools"],
            json!(["visma_init"])
        );
        assert!(
            by_id("receivables-aging")
                .get("unavailable_tools")
                .is_none(),
            "everything it needs is here"
        );
        assert_eq!(
            by_id("edit-order")["unavailable_tools"],
            json!(["visma_salesorder_patch_plan"])
        );

        let g = GetSkill::new(idx, only_query);
        let one = g.call(json!({"id": "edit-order"})).await.unwrap();
        assert_eq!(
            one["skill"]["unavailable_tools"],
            json!(["visma_salesorder_patch_plan"])
        );
        assert!(one["availability_note"]
            .as_str()
            .unwrap()
            .contains("visma_init"));
        let fine = g.call(json!({"id": "receivables-aging"})).await.unwrap();
        assert!(fine.get("availability_note").is_none());
    }
}
