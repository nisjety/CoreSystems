//! Accuracy gates for the bundled skills. A skill that drifts from the tools (renamed argument,
//! removed endpoint, SQL the guard would refuse) fails here instead of misleading a model.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::skills::{Example, SkillIndex, MAX_SKILL_BYTES};
use crate::testutil::{harness, is_warehouse_dataset, real_catalog};
use crate::warehouse::DEFAULT_PROJECT;

fn index() -> &'static SkillIndex {
    SkillIndex::embedded()
}

fn tool_names() -> BTreeSet<String> {
    harness()
        .registry
        .names()
        .iter()
        .map(|n| (*n).to_string())
        .collect()
}

/// Just enough JSON Schema for our tool schemas: type, enum, minimum/maximum, required, properties,
/// additionalProperties:false, items.
pub(crate) fn validate(schema: &Value, v: &Value, path: &str, errs: &mut Vec<String>) {
    if let Some(t) = schema["type"].as_str() {
        let ok = match t {
            "object" => v.is_object(),
            "array" => v.is_array(),
            "string" => v.is_string(),
            "boolean" => v.is_boolean(),
            "integer" => v.is_i64() || v.is_u64(),
            "number" => v.is_number(),
            _ => true,
        };
        if !ok {
            errs.push(format!("{path}: expected {t}, got {v}"));
            return;
        }
    }
    if let Some(en) = schema["enum"].as_array() {
        if !en.contains(v) {
            errs.push(format!("{path}: {v} is not one of {en:?}"));
        }
    }
    if let (Some(n), Some(min)) = (v.as_f64(), schema["minimum"].as_f64()) {
        if n < min {
            errs.push(format!("{path}: {n} is below the minimum {min}"));
        }
    }
    if let (Some(n), Some(max)) = (v.as_f64(), schema["maximum"].as_f64()) {
        if n > max {
            errs.push(format!("{path}: {n} is above the maximum {max}"));
        }
    }
    if let Some(obj) = v.as_object() {
        for req in schema["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !obj.contains_key(req) {
                errs.push(format!("{path}: missing required `{req}`"));
            }
        }
        let props = schema["properties"].as_object();
        for (k, val) in obj {
            match props.and_then(|p| p.get(k)) {
                Some(sub) => validate(sub, val, &format!("{path}.{k}"), errs),
                None if schema["additionalProperties"] == Value::Bool(false) => {
                    errs.push(format!("{path}: unknown property `{k}`"));
                }
                None => {}
            }
        }
    }
    if let (Some(items), Some(sub)) = (v.as_array(), schema.get("items")) {
        for (i, it) in items.iter().enumerate() {
            validate(sub, it, &format!("{path}[{i}]"), errs);
        }
    }
}

fn all_examples() -> Vec<(String, Example)> {
    index()
        .all()
        .iter()
        .flat_map(|s| {
            let ex = s.examples().unwrap_or_else(|e| panic!("{e}"));
            ex.into_iter()
                .map(|e| (s.id.clone(), e))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn is_kebab(id: &str) -> bool {
    !id.is_empty()
        && id.split('-').all(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

#[test]
fn skills_are_well_formed() {
    assert!(!index().all().is_empty());
    for s in index().all() {
        assert!(is_kebab(&s.id), "{}: id must be kebab-case", s.id);
        assert!(s.title.len() >= 4, "{}: title too short", s.id);
        assert!(
            s.description.len() >= 30,
            "{}: description should say when to use the skill",
            s.id
        );
        assert!(!s.tags.is_empty(), "{}: needs tags", s.id);
        assert!(!s.tools.is_empty(), "{}: needs related tools", s.id);
        assert!(s.body.len() >= 300, "{}: body is a stub", s.id);
        assert!(
            s.body.len() <= MAX_SKILL_BYTES,
            "{}: body is {} bytes, limit {MAX_SKILL_BYTES}",
            s.id,
            s.body.len()
        );
        assert!(
            !s.body.contains("TODO") && !s.body.contains("placeholder"),
            "{}: unfinished",
            s.id
        );
    }
}

#[test]
fn skills_reference_only_real_tools_and_real_skills_without_cycles() {
    let names = tool_names();
    let ids: BTreeSet<&str> = index().all().iter().map(|s| s.id.as_str()).collect();
    for s in index().all() {
        for t in &s.tools {
            assert!(
                names.contains(t),
                "{}: related tool `{t}` does not exist (tools: {names:?})",
                s.id
            );
        }
        for t in &s.primary {
            assert!(
                s.tools.contains(t),
                "{}: primary tool `{t}` must also be listed in tools",
                s.id
            );
        }
        for p in &s.prerequisites {
            assert!(
                ids.contains(p.as_str()),
                "{}: prerequisite `{p}` does not exist",
                s.id
            );
            assert_ne!(p, &s.id, "{}: is its own prerequisite", s.id);
        }
    }
    // no prerequisite cycles
    let graph: BTreeMap<&str, Vec<&str>> = index()
        .all()
        .iter()
        .map(|s| {
            (
                s.id.as_str(),
                s.prerequisites.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    fn visit<'a>(
        n: &'a str,
        g: &BTreeMap<&'a str, Vec<&'a str>>,
        stack: &mut Vec<&'a str>,
        done: &mut BTreeSet<&'a str>,
    ) {
        assert!(
            !stack.contains(&n),
            "prerequisite cycle through {n}: {stack:?}"
        );
        if done.contains(n) {
            return;
        }
        stack.push(n);
        for m in &g[n] {
            visit(m, g, stack, done);
        }
        stack.pop();
        done.insert(n);
    }
    let mut done = BTreeSet::new();
    for n in graph.keys() {
        visit(n, &graph, &mut vec![], &mut done);
    }
}

#[test]
fn every_tool_has_a_skill_list_with_a_primary_skill() {
    for name in tool_names() {
        let skills = index().for_tool(&name);
        assert!(
            !skills.is_empty(),
            "tool `{name}` has no skills: add it to the `tools:` list of a skill"
        );
        assert!(
            skills[0].primary.iter().any(|t| t == &name),
            "tool `{name}` has skills but none marks it `primary` (the one to read first): {:?}",
            skills.iter().map(|s| &s.id).collect::<Vec<_>>()
        );
    }
}

#[test]
fn prose_only_names_real_tools() {
    let names = tool_names();
    let tables: BTreeSet<String> = real_catalog()
        .all()
        .iter()
        .filter_map(|e| e.bigquery_table.clone())
        .collect();
    // column names that legitimately start with the same prefixes
    let columns = [
        "warehouse_key",
        "warehouse_id",
        "warehouse_name",
        "warehouse_location_id",
    ];
    for s in index().all() {
        for word in s
            .body
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        {
            let candidate = word.starts_with("visma_") || word.starts_with("warehouse_");
            if candidate
                && !names.contains(word)
                && !tables.contains(word)
                && !columns.contains(&word)
                && !is_warehouse_dataset(word)
            {
                panic!("skill {} mentions `{word}`, which is neither a tool, a catalog table, a warehouse dataset nor a known column", s.id);
            }
        }
    }
}

/// Examples whose arguments reference runtime state (a plan or result handle from an earlier call):
/// only their shape can be checked.
const SCHEMA_ONLY: &[&str] = &[
    "visma_read_result",
    "visma_salesorder_patch_execute",
    "visma_salesorder_create_execute",
    "visma_salesorder_status_execute",
];

#[tokio::test]
async fn every_example_conforms_to_its_schema_and_runs_through_the_tool_code_with_fake_backends() {
    let examples = all_examples();
    assert!(!examples.is_empty(), "skills need worked examples");
    let h = harness();
    let defs: BTreeMap<&'static str, Value> = h
        .registry
        .defs()
        .into_iter()
        .map(|d| (d.name, d.input_schema))
        .collect();
    for (skill, ex) in examples {
        let schema = defs.get(ex.tool.as_str()).unwrap_or_else(|| {
            panic!(
                "{skill}:{}: example calls unknown tool `{}`",
                ex.line, ex.tool
            )
        });
        let mut errs = vec![];
        validate(schema, &ex.args, "args", &mut errs);
        assert!(
            errs.is_empty(),
            "{skill}:{}: `{}` example violates its schema: {errs:?}",
            ex.line,
            ex.tool
        );
        if SCHEMA_ONLY.contains(&ex.tool.as_str()) {
            continue;
        }
        h.registry
            .call(&ex.tool, ex.args.clone())
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "{skill}:{}: `{}` example fails in the real tool: {e}\nargs: {}",
                    ex.line, ex.tool, ex.args
                )
            });
    }
    // The examples really reached the backends, not only the schema layer.
    assert!(
        !h.calls.sql.lock().unwrap().is_empty(),
        "no SQL example reached the warehouse"
    );
    assert!(
        h.calls
            .erp
            .lock()
            .unwrap()
            .iter()
            .any(|(_, path, _)| path.contains("customerinvoice")),
        "no live ERP example reached the ERP client"
    );
    assert!(
        !h.calls.gql.lock().unwrap().is_empty(),
        "no DataMart example reached the GraphQL client"
    );
    assert!(
        h.calls
            .reads
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.contains("/api/v3/SalesOrders/")),
        "no sales order example reached the SalesOrder reader"
    );
}

#[test]
fn every_sql_example_is_a_single_read_only_statement_over_allowed_datasets() {
    // Executed through the real SafeWarehouse above; this adds the cheap structural checks.
    let mut seen = 0;
    for (skill, ex) in all_examples() {
        if ex.tool != "warehouse_query" {
            continue;
        }
        seen += 1;
        let sql = ex.args["sql"]
            .as_str()
            .unwrap_or_else(|| panic!("{skill}:{}: warehouse_query example without sql", ex.line));
        assert!(
            !sql.to_lowercase().contains("select *"),
            "{skill}:{}: `SELECT *` in an example (select the columns you need)",
            ex.line
        );
        assert!(
            sql.contains(&format!("{DEFAULT_PROJECT}.")),
            "{skill}:{}: fully qualify tables as `{DEFAULT_PROJECT}.dataset.table`",
            ex.line
        );
        // The offline harness only sees backticked tables (BigQuery reports the real ones in
        // production), so a dataset or project named outside backticks would dodge its policy check.
        for (i, outside) in sql.split('`').enumerate().filter(|(i, _)| i % 2 == 0) {
            let named = outside
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
                .find(|w| {
                    *w == DEFAULT_PROJECT || is_warehouse_dataset(w) || w.starts_with("dw_staging_")
                });
            assert!(
                named.is_none(),
                "{skill}:{}: a dataset or project outside backticks (chunk {i}): {outside:?}",
                ex.line
            );
        }
    }
    assert!(
        seen >= 40,
        "expected the bundled warehouse_query examples, saw {seen}"
    );
}

#[test]
fn the_skills_in_the_binary_are_the_skills_on_disk() {
    // Guards the build step: an unreadable directory or a stale embed must not ship fewer skills.
    let dir = format!("{}/skills", env!("CARGO_MANIFEST_DIR"));
    let on_disk: BTreeSet<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {dir}: {e}"))
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            e.file_name()
                .to_str()
                .and_then(|n| n.strip_suffix(".md"))
                .map(String::from)
        })
        .collect();
    let embedded: BTreeSet<String> = index().all().iter().map(|s| s.id.clone()).collect();
    assert_eq!(
        embedded, on_disk,
        "the embedded skills differ from skills/*.md"
    );
    assert!(
        embedded.len() >= 19,
        "expected the bundled skills, found {}",
        embedded.len()
    );
}

#[tokio::test]
async fn the_words_the_server_tells_every_client_name_real_tools() {
    // The connection instructions and `visma_init`'s ground rules are read by the model before
    // anything else: a tool name in them that does not exist would mislead it from the first call.
    let h = harness();
    let names = tool_names();
    let tables: BTreeSet<String> = real_catalog()
        .all()
        .iter()
        .filter_map(|e| e.bigquery_table.clone())
        .collect();
    let server = crate::McpServer::new(h.registry.clone());
    let init = h
        .registry
        .call("visma_init", serde_json::json!({}))
        .await
        .unwrap();
    let texts = [
        server.instructions().to_string(),
        init["ground_rules"].to_string(),
        init["known_issues"].to_string(),
    ];
    for text in texts {
        for word in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            if (word.starts_with("visma_") || word.starts_with("warehouse_"))
                && !is_warehouse_dataset(word)
                && !tables.contains(word)
            {
                assert!(
                    names.contains(word),
                    "startup text names `{word}`, which is not a tool, a dataset or a catalog table"
                );
            }
        }
    }
}

#[tokio::test]
async fn nothing_a_model_reads_names_the_old_warehouse() {
    // Built from pieces so that this guard does not itself name them anywhere in the crate.
    let old_project = concat!("prj-dw", "-dev");
    let old_datasets = concat!("dw_", "1_");
    let h = harness();
    let mut texts: Vec<(String, String)> = Vec::new();
    for s in index().all() {
        texts.push((
            format!("skill {}", s.id),
            format!(
                "{} {} {} {}",
                s.title,
                s.description,
                s.tags.join(" "),
                s.body
            ),
        ));
    }
    for d in h.registry.defs() {
        texts.push((format!("tool {}", d.name), d.description.clone()));
    }
    let init = h
        .registry
        .call("visma_init", serde_json::json!({}))
        .await
        .unwrap();
    texts.push(("visma_init".into(), init.to_string()));
    texts.push((
        "server instructions".into(),
        crate::McpServer::new(h.registry.clone())
            .instructions()
            .to_string(),
    ));
    for (what, text) in texts {
        assert!(
            !text.contains(old_project) && !text.contains(old_datasets),
            "{what} still names the old warehouse"
        );
    }
}

#[test]
fn skills_that_query_the_warehouse_name_the_v2_project_and_its_datasets() {
    // The datasets a reader of the skills needs to find are named, by their v2 names, in at least one
    // runnable example each.
    let sql: String = all_examples()
        .iter()
        .filter(|(_, e)| e.tool == "warehouse_query")
        .filter_map(|(_, e)| e.args["sql"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    for ds in [
        "visma_gold_common",
        "visma_gold_finance",
        "visma_gold_sales",
        "visma_datamart",
        "visma_silver",
        "visma_silver_common",
        "visma_bronze",
        "dw_staging_visma",
    ] {
        assert!(
            sql.contains(&format!("{DEFAULT_PROJECT}.{ds}")),
            "no SQL example reads {ds}"
        );
    }
}

#[test]
fn skills_name_themselves_consistently_in_search() {
    // Every skill must be findable by its own title: protects against search regressions.
    for s in index().all() {
        let hits = index().search(&s.title);
        assert_eq!(
            hits.first().map(|(h, _)| h.id.as_str()),
            Some(s.id.as_str()),
            "searching {:?} should return {} first",
            s.title,
            s.id
        );
    }
}
