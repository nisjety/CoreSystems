//! Skills: short, tested usage guides for this server's tools.
//!
//! A skill is a Markdown file with front matter (`id`, `title`, `description`, `tags`, `tools`,
//! `prerequisites`, `difficulty`, `steps`). `tools` is the skill's list of related tools; inverting
//! it gives every tool its own skill list, which is shown in the tool's description and `_meta`.
//!
//! Examples live in ```` ```tool-call ```` blocks (first line = tool name, then a JSON argument
//! object). Tests validate every example against the real tool schema, the SQL guard, the GraphQL
//! guard and the endpoint catalog, and an opt-in live test dry-runs every SQL example against
//! BigQuery, so a skill cannot silently rot when a table or tool changes.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::{json, Value};

include!(concat!(env!("OUT_DIR"), "/embedded_skills.rs"));

/// Upper bound for one skill body; keeps retrieval cheap for the model's context.
pub const MAX_SKILL_BYTES: usize = 16_000;

const DIFFICULTIES: &[&str] = &["beginner", "intermediate", "advanced"];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FrontMatter {
    id: String,
    title: String,
    description: String,
    #[serde(default)]
    tags: Vec<String>,
    /// Tools this skill is about.
    #[serde(default)]
    tools: Vec<String>,
    /// Tools for which this is the *first* skill to read.
    #[serde(default)]
    primary: Vec<String>,
    #[serde(default)]
    prerequisites: Vec<String>,
    #[serde(default = "default_difficulty")]
    difficulty: String,
    #[serde(default = "default_steps")]
    steps: u32,
}

fn default_difficulty() -> String {
    "intermediate".into()
}

fn default_steps() -> u32 {
    3
}

#[derive(Debug, Clone)]
pub struct Skill {
    pub id: String,
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
    pub tools: Vec<String>,
    pub primary: Vec<String>,
    pub prerequisites: Vec<String>,
    pub difficulty: String,
    pub steps: u32,
    pub body: String,
}

/// One `tool-call` block found in a skill body.
#[derive(Debug, Clone, PartialEq)]
pub struct Example {
    pub tool: String,
    pub args: Value,
    /// 1-based line of the opening fence inside the body.
    pub line: usize,
}

impl Skill {
    fn parse(file: &str, text: &str) -> Result<Self, String> {
        // An editor on another platform may save a BOM and CRLF line endings.
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let unix;
        let text = if text.contains('\r') {
            unix = text.replace("\r\n", "\n");
            unix.as_str()
        } else {
            text
        };
        let rest = text
            .strip_prefix("---\n")
            .ok_or_else(|| format!("{file}: must start with a `---` front-matter line"))?;
        let (front, body) = rest
            .split_once("\n---\n")
            .ok_or_else(|| format!("{file}: front matter is not closed by a `---` line"))?;
        let fm: FrontMatter =
            serde_yaml::from_str(front).map_err(|e| format!("{file}: front matter: {e}"))?;
        let expected = file.strip_suffix(".md").unwrap_or(file);
        if fm.id != expected {
            return Err(format!(
                "{file}: id `{}` must equal the file name `{expected}`",
                fm.id
            ));
        }
        if !DIFFICULTIES.contains(&fm.difficulty.as_str()) {
            return Err(format!(
                "{file}: difficulty `{}` must be one of {DIFFICULTIES:?}",
                fm.difficulty
            ));
        }
        if fm.steps == 0 {
            return Err(format!("{file}: steps must be at least 1"));
        }
        Ok(Self {
            id: fm.id,
            title: fm.title,
            description: fm.description,
            tags: fm.tags,
            tools: fm.tools,
            primary: fm.primary,
            prerequisites: fm.prerequisites,
            difficulty: fm.difficulty,
            steps: fm.steps,
            body: body.trim().to_string(),
        })
    }

    /// Metadata only (no body).
    pub fn summary(&self) -> Value {
        json!({
            "id": self.id,
            "title": self.title,
            "description": self.description,
            "tags": self.tags,
            "related_tools": self.tools,
            "prerequisites": self.prerequisites,
            "difficulty": self.difficulty,
            "estimated_steps": self.steps,
        })
    }

    pub fn full(&self) -> Value {
        let mut v = self.summary();
        v["content"] = json!(self.body);
        v
    }

    /// Every `tool-call` block, in order. A malformed block is an error (tests surface it).
    pub fn examples(&self) -> Result<Vec<Example>, String> {
        let mut out = Vec::new();
        let mut lines = self.body.lines().enumerate();
        while let Some((i, line)) = lines.next() {
            if line.trim() != "```tool-call" {
                continue;
            }
            let mut block: Vec<&str> = Vec::new();
            let mut closed = false;
            for (_, l) in lines.by_ref() {
                if l.trim() == "```" {
                    closed = true;
                    break;
                }
                block.push(l);
            }
            if !closed {
                return Err(format!(
                    "{}: unterminated tool-call block at line {}",
                    self.id,
                    i + 1
                ));
            }
            let mut it = block.into_iter().skip_while(|l| l.trim().is_empty());
            let tool = it
                .next()
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .ok_or_else(|| {
                    format!(
                        "{}: tool-call block at line {} has no tool name",
                        self.id,
                        i + 1
                    )
                })?;
            let json_text: String = it.collect::<Vec<_>>().join("\n");
            let args: Value = if json_text.trim().is_empty() {
                json!({})
            } else {
                serde_json::from_str(&json_text).map_err(|e| {
                    format!(
                        "{}: tool-call block at line {}: invalid JSON: {e}",
                        self.id,
                        i + 1
                    )
                })?
            };
            out.push(Example {
                tool: tool.to_string(),
                args,
                line: i + 1,
            });
        }
        Ok(out)
    }
}

#[derive(Debug, Clone)]
pub struct SkillIndex {
    skills: Vec<Skill>,
}

const STOPWORDS: &[&str] = &[
    "the", "a", "an", "and", "or", "of", "to", "for", "in", "on", "how", "do", "i", "is", "with",
    "my", "me",
];

/// Longest query that is searched; a longer one is cut (a query is client input).
const MAX_QUERY_CHARS: usize = 200;
/// Most distinct words of a query that take part in the ranking.
const MAX_QUERY_WORDS: usize = 12;

/// A lowercase word with a trailing plural `s` removed, so "invoices" meets "invoice". Applied to the
/// query and to the indexed text alike, so the two always agree.
fn stem(w: &str) -> String {
    match w.strip_suffix('s') {
        Some(rest) if w.chars().count() > 3 && !w.ends_with("ss") => rest.to_string(),
        _ => w.to_string(),
    }
}

/// Alphanumeric words of `text`, lowercased and stemmed (underscores and hyphens split words, so a
/// tool name or an id contributes its parts).
fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| stem(&w.to_lowercase()))
}

/// The words of a query: at least two characters, no stopwords, no repeats, and bounded in number.
fn query_words(q: &str) -> Vec<String> {
    let q: String = q.chars().take(MAX_QUERY_CHARS).collect();
    let mut out: Vec<String> = Vec::new();
    for w in q
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 2 && !STOPWORDS.contains(w))
    {
        let w = stem(w);
        if !out.contains(&w) {
            out.push(w);
        }
        if out.len() == MAX_QUERY_WORDS {
            break;
        }
    }
    out
}

impl SkillIndex {
    /// Parse `(file name, contents)` pairs. Structural problems are errors: a broken skill must
    /// fail the build's tests, never be served half-parsed.
    pub fn parse(files: &[(&str, &str)]) -> Result<Self, String> {
        let mut skills = Vec::with_capacity(files.len());
        for (name, text) in files {
            skills.push(Skill::parse(name, text)?);
        }
        skills.sort_by(|a, b| a.id.cmp(&b.id));
        let mut seen = BTreeSet::new();
        for s in &skills {
            if !seen.insert(s.id.clone()) {
                return Err(format!("duplicate skill id {}", s.id));
            }
        }
        Ok(Self { skills })
    }

    /// The skills compiled into this binary.
    pub fn embedded() -> &'static SkillIndex {
        static INDEX: OnceLock<SkillIndex> = OnceLock::new();
        INDEX.get_or_init(|| {
            SkillIndex::parse(EMBEDDED_SKILLS)
                .unwrap_or_else(|e| panic!("embedded skills are invalid: {e}"))
        })
    }

    pub fn all(&self) -> &[Skill] {
        &self.skills
    }

    pub fn get(&self, id: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.id == id)
    }

    pub fn tags(&self) -> Vec<String> {
        let set: BTreeSet<&String> = self.skills.iter().flat_map(|s| s.tags.iter()).collect();
        set.into_iter().cloned().collect()
    }

    /// The skills for one tool, best first: those that name it as primary, then the more specific
    /// ones (a skill about two tools says more about each than one about thirteen), then by id.
    /// The tool description shows only the first few, so this order decides which ones a model sees.
    pub fn for_tool(&self, tool: &str) -> Vec<&Skill> {
        let mut v: Vec<&Skill> = self
            .skills
            .iter()
            .filter(|s| s.tools.iter().any(|t| t == tool))
            .collect();
        v.sort_by_key(|s| {
            (
                !s.primary.iter().any(|t| t == tool),
                s.tools.len(),
                s.id.clone(),
            )
        });
        v
    }

    /// tool -> skill ids (primary first).
    pub fn by_tool(&self) -> BTreeMap<String, Vec<String>> {
        let tools: BTreeSet<&String> = self.skills.iter().flat_map(|s| s.tools.iter()).collect();
        tools
            .into_iter()
            .map(|t| {
                (
                    t.clone(),
                    self.for_tool(t).into_iter().map(|s| s.id.clone()).collect(),
                )
            })
            .collect()
    }

    /// Ranked free-text search. Whole words only (a substring match made "AR aging" find the
    /// DataMart skill because "datamart" contains "ar"): a word in the id scores most, then the title,
    /// the tags, the description, the related tool names and the body (counted up to three times).
    /// An exact id, with spaces for hyphens, beats everything.
    pub fn search(&self, query: &str) -> Vec<(&Skill, u32)> {
        let wanted = query_words(query);
        if wanted.is_empty() {
            return vec![];
        }
        let as_id = query.trim().to_lowercase().replace(' ', "-");
        let mut scored: Vec<(&Skill, u32)> = self
            .skills
            .iter()
            .filter_map(|s| {
                let id: BTreeSet<String> = words(&s.id).collect();
                let title: BTreeSet<String> = words(&s.title).collect();
                let tags: BTreeSet<String> = s.tags.iter().flat_map(|t| words(t)).collect();
                let desc: BTreeSet<String> = words(&s.description).collect();
                let tools: BTreeSet<String> = s.tools.iter().flat_map(|t| words(t)).collect();
                let mut body: BTreeMap<String, u32> = BTreeMap::new();
                for w in words(&s.body) {
                    *body.entry(w).or_default() += 1;
                }
                let mut score = if as_id == s.id { 50 } else { 0 };
                for w in &wanted {
                    score += [(&id, 6), (&title, 5), (&tags, 4), (&desc, 3), (&tools, 2)]
                        .iter()
                        .filter(|(set, _)| set.contains(w))
                        .map(|(_, points)| points)
                        .sum::<u32>();
                    score += body.get(w).copied().unwrap_or(0).min(3);
                }
                (score > 0).then_some((s, score))
            })
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.id.cmp(&b.0.id)));
        scored
    }
}

/// How many skill ids a tool description names before pointing at `visma_list_skills`.
const DESCRIPTION_SKILLS: usize = 4;

/// The sentence appended to a tool's description so the model knows its skills.
pub fn description_suffix(tool: &str, ids: &[String]) -> String {
    if ids.is_empty() {
        return String::new();
    }
    let shown: Vec<String> = ids
        .iter()
        .take(DESCRIPTION_SKILLS)
        .map(|i| format!("`{i}`"))
        .collect();
    let more = ids.len().saturating_sub(DESCRIPTION_SKILLS);
    let tail = if more > 0 {
        format!(
            " (+{more} more: {}(tool=\"{tool}\"))",
            crate::tools::skills::LIST
        )
    } else {
        String::new()
    };
    // The tools you use to start or to read skills cannot ask you to read a skill first.
    let bootstrap = [
        crate::tools::init::NAME,
        crate::tools::skills::LIST,
        crate::tools::skills::GET,
    ];
    if bootstrap.contains(&tool) {
        return format!(
            "\n\nSkills for this tool: {}{tail}. Background only: you can call this tool without reading them first.",
            shown.join(", ")
        );
    }
    format!(
        "\n\nSkills for this tool: {}{tail}. Read the first one with {} before using it for the first time.",
        shown.join(", "),
        crate::tools::skills::GET
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "---\nid: demo\ntitle: Demo\ndescription: A demo skill\ntags: [a, b]\ntools: [t1, t2]\nprimary: [t1]\n---\n# Body\n\ntext\n";

    #[test]
    fn parses_front_matter_and_body_and_applies_defaults() {
        let idx = SkillIndex::parse(&[("demo.md", GOOD)]).unwrap();
        let s = idx.get("demo").unwrap();
        assert_eq!(
            (s.title.as_str(), s.difficulty.as_str(), s.steps),
            ("Demo", "intermediate", 3)
        );
        assert_eq!(s.tools, ["t1", "t2"]);
        assert!(s.body.starts_with("# Body"));
        assert_eq!(s.full()["content"], s.body);
        assert!(s.summary().get("content").is_none());
    }

    #[test]
    fn structural_problems_are_errors_not_silently_skipped() {
        assert!(SkillIndex::parse(&[("x.md", "no front matter")]).is_err());
        assert!(
            SkillIndex::parse(&[("x.md", "---\nid: x\ntitle: T\n")]).is_err(),
            "unclosed"
        );
        assert!(
            SkillIndex::parse(&[("x.md", "---\nid: other\ntitle: T\ndescription: d\n---\nb")])
                .unwrap_err()
                .contains("must equal the file name")
        );
        assert!(
            SkillIndex::parse(&[(
                "x.md",
                "---\nid: x\ntitle: T\ndescription: d\nbogus: 1\n---\nb"
            )])
            .is_err(),
            "unknown keys rejected"
        );
        assert!(SkillIndex::parse(&[(
            "x.md",
            "---\nid: x\ntitle: T\ndescription: d\ndifficulty: impossible\n---\nb"
        )])
        .is_err());
        assert!(SkillIndex::parse(&[(
            "x.md",
            "---\nid: x\ntitle: T\ndescription: d\nsteps: 0\n---\nb"
        )])
        .is_err());
        let a = GOOD.replace("demo", "a");
        assert!(
            SkillIndex::parse(&[("a.md", &a), ("b.md", &a)]).is_err(),
            "id/file mismatch"
        );
        let err = SkillIndex::parse(&[("a.md", &a), ("a.md", &a)]).unwrap_err();
        assert!(err.contains("duplicate skill id a"), "{err}");
    }

    #[test]
    fn inverts_tools_to_per_tool_skill_lists_with_primary_first() {
        let mk = |id: &str, tools: &str, primary: &str| {
            format!("---\nid: {id}\ntitle: {id}\ndescription: d\ntools: [{tools}]\nprimary: [{primary}]\n---\nbody")
        };
        let (a, b, c) = (
            mk("a-skill", "t1", ""),
            mk("b-skill", "t1, t2", "t1"),
            mk("c-skill", "t2", ""),
        );
        let idx = SkillIndex::parse(&[("a-skill.md", &a), ("b-skill.md", &b), ("c-skill.md", &c)])
            .unwrap();
        assert_eq!(
            idx.by_tool()["t1"],
            ["b-skill", "a-skill"],
            "primary first, then the more specific skill"
        );
        assert_eq!(
            idx.by_tool()["t2"],
            ["c-skill", "b-skill"],
            "no primary: the skill about one tool before the skill about two"
        );
        let same = |id: &str| mk(id, "t3, t4", "");
        let (x, y) = (same("y-skill"), same("x-skill"));
        let idx2 = SkillIndex::parse(&[("y-skill.md", &x), ("x-skill.md", &y)]).unwrap();
        assert_eq!(
            idx2.by_tool()["t3"],
            ["x-skill", "y-skill"],
            "equal specificity: by id"
        );
        assert!(idx.for_tool("none").is_empty());
    }

    #[test]
    fn extracts_tool_call_examples_and_rejects_malformed_blocks() {
        let body = "text\n```tool-call\nwarehouse_query\n{\"sql\": \"SELECT 1\",\n \"max_rows\": 5}\n```\nmore\n```tool-call\nvisma_list_tenants\n```\n";
        let s = Skill::parse(
            "e.md",
            &format!("---\nid: e\ntitle: E\ndescription: d\n---\n{body}"),
        )
        .unwrap();
        let ex = s.examples().unwrap();
        assert_eq!(ex.len(), 2);
        assert_eq!(
            (
                ex[0].tool.as_str(),
                ex[0].args["max_rows"].as_i64(),
                ex[0].line
            ),
            ("warehouse_query", Some(5), 2)
        );
        assert_eq!(ex[1].args, json!({}), "no JSON means no arguments");
        for bad in [
            "```tool-call\n```\n",
            "```tool-call\nt\n{not json}\n```\n",
            "```tool-call\nt\n{}\n",
        ] {
            let s = Skill::parse(
                "e.md",
                &format!("---\nid: e\ntitle: E\ndescription: d\n---\n{bad}"),
            )
            .unwrap();
            assert!(s.examples().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn search_ranks_id_and_title_above_body_and_ignores_stopwords() {
        let mk = |id: &str, title: &str, body: &str| {
            format!(
                "---\nid: {id}\ntitle: {title}\ndescription: about things\ntags: [x]\n---\n{body}"
            )
        };
        let (a, b) = (
            mk("receivables-aging", "Receivables aging", "overdue"),
            mk("sales-reporting", "Sales", "aging aging aging aging"),
        );
        let idx =
            SkillIndex::parse(&[("receivables-aging.md", &a), ("sales-reporting.md", &b)]).unwrap();
        let r = idx.search("how do I do aging of the receivables");
        assert_eq!(r[0].0.id, "receivables-aging");
        assert!(idx.search("the of a").is_empty(), "only stopwords");
        assert!(idx.search("zzzz").is_empty());
    }

    #[test]
    fn description_suffix_names_the_first_skills_and_points_at_the_rest() {
        let ids: Vec<String> = (1..=6).map(|i| format!("s{i}")).collect();
        let t = description_suffix("warehouse_query", &ids);
        assert!(
            t.contains("`s1`, `s2`, `s3`, `s4`") && !t.contains("`s5`"),
            "{t}"
        );
        assert!(
            t.contains("+2 more") && t.contains("tool=\"warehouse_query\""),
            "{t}"
        );
        assert_eq!(description_suffix("x", &[]), "");
    }

    #[test]
    fn the_bootstrap_tools_are_not_told_to_read_a_skill_before_they_are_used() {
        let ids = vec!["start-here".to_string()];
        for tool in ["visma_init", "visma_list_skills", "visma_get_skill"] {
            let t = description_suffix(tool, &ids);
            assert!(
                t.contains("without reading them first") && !t.contains("before using it"),
                "{t}"
            );
        }
        assert!(description_suffix("warehouse_query", &ids).contains("before using it"));
    }

    #[test]
    fn search_matches_whole_words_so_ar_is_not_found_inside_datamart() {
        let idx = SkillIndex::embedded();
        for q in [
            "AR aging",
            "ar",
            "AR",
            "overdue invoices aging",
            "receivables",
        ] {
            assert_eq!(idx.search(q)[0].0.id, "receivables-payables", "query {q:?}");
        }
        let top: Vec<&str> = idx
            .search("ic sales")
            .iter()
            .take(2)
            .map(|(s, _)| s.id.as_str())
            .collect();
        assert!(top.contains(&"sales-reporting"), "{top:?}");
        // an exact id, with spaces for hyphens, beats any other match
        assert_eq!(idx.search("data freshness")[0].0.id, "data-freshness");
        assert_eq!(idx.search("sales-reporting")[0].0.id, "sales-reporting");
        // plural and singular meet
        assert_eq!(
            idx.search("project margin")[0].0.id,
            idx.search("projects margins")[0].0.id
        );
    }

    #[test]
    fn a_hostile_query_is_bounded_in_work_and_still_answers() {
        let idx = SkillIndex::embedded();
        let started = std::time::Instant::now();
        let huge =
            "invoice ".repeat(100_000) + &(0..5_000).map(|i| format!("w{i} ")).collect::<String>();
        let r = idx.search(&huge);
        assert!(
            !r.is_empty(),
            "the part that is searched still finds skills"
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "search must not scale with the query"
        );
        // only the first MAX_QUERY_CHARS characters count, and repeated words collapse
        assert_eq!(query_words(&huge), vec!["invoice".to_string()]);
        let many: String = (0..100).map(|i| format!("word{i} ")).collect();
        assert_eq!(
            query_words(&many).len(),
            MAX_QUERY_WORDS.min(query_words(&many[..many.len().min(MAX_QUERY_CHARS)]).len())
        );
        assert!(query_words(&many).len() <= MAX_QUERY_WORDS);
    }

    #[test]
    fn front_matter_survives_a_bom_and_windows_line_endings() {
        let good =
            "---\nid: demo\ntitle: Demo\ndescription: d\ntags: [x]\ntools: [t]\n---\nbody text\n";
        let crlf = format!("\u{feff}{}", good.replace('\n', "\r\n"));
        let s = Skill::parse("demo.md", &crlf).unwrap();
        assert_eq!((s.id.as_str(), s.body.as_str()), ("demo", "body text"));
    }
}
