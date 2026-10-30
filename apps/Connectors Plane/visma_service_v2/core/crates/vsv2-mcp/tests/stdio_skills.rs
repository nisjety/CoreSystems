//! Black-box checks of the skills surface through the real binary over stdio, with no credentials:
//! `visma_init`, per-tool skill lists, skill tools, skill resources and `--print-manifest`.

mod common;

use std::collections::BTreeSet;
use std::process::Command;

use common::{repo_endpoint_dir, McpClient};
use serde_json::{json, Value};
use vsv2_mcp::warehouse::{DEFAULT_DATASETS, DEFAULT_PROJECT};

fn data(r: &Value) -> &Value {
    assert_ne!(r["result"]["isError"], true, "tool failed: {r}");
    &r["result"]["structuredContent"]
}

fn skills_dir_ids() -> BTreeSet<String> {
    let dir = format!("{}/skills", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_dir(dir)
        .expect("skills directory")
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.strip_suffix(".md").map(String::from)
        })
        .collect()
}

#[tokio::test]
async fn visma_init_explains_the_server_and_points_at_start_here() {
    let mut c = McpClient::spawn(&[]).await;
    let r = c.call("visma_init", json!({})).await;
    let d = data(&r);
    assert_eq!(d["start_here"], json!(["start-here"]));
    assert!(d["acts_as"]
        .as_str()
        .unwrap()
        .contains("service credentials"));
    // Nothing is configured here, so each missing capability says why.
    assert_eq!(d["capabilities"]["warehouse"]["enabled"], false);
    assert!(d["capabilities"]["warehouse"]["reason"]
        .as_str()
        .unwrap()
        .contains("VSV2_MCP_BQ_PROJECT"));
    assert_eq!(d["capabilities"]["writes"]["enabled"], false);
    assert_eq!(
        d["skills"]["count"].as_u64().unwrap() as usize,
        skills_dir_ids().len()
    );
    assert!(
        d["prompts"].as_array().unwrap().is_empty(),
        "no warehouse and no write tools: no workflow can be followed: {d}"
    );
    assert!(!d["ground_rules"].as_array().unwrap().is_empty());
    assert!(!d["known_issues"]["issues"].as_array().unwrap().is_empty());

    // With a warehouse configured the analytic workflows appear; the edit workflow still needs writes.
    let mut w = McpClient::spawn(&[("VSV2_MCP_BQ_PROJECT", "some-project")]).await;
    let r = w.call("visma_init", json!({})).await;
    let prompts = data(&r)["prompts"].as_array().unwrap().clone();
    assert!(
        prompts.iter().any(|p| p == "period_end_review") && prompts.len() == 7,
        "{prompts:?}"
    );
    assert!(!prompts.iter().any(|p| p == "edit_sales_order"));
}

#[tokio::test]
async fn visma_init_and_the_tool_descriptions_show_the_v2_warehouse() {
    // Not configured: the reason says which project to set.
    let mut c = McpClient::spawn(&[]).await;
    let r = c.call("visma_init", json!({})).await;
    let reason = data(&r)["capabilities"]["warehouse"]["reason"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        reason.contains("VSV2_MCP_BQ_PROJECT") && reason.contains(DEFAULT_PROJECT),
        "{reason}"
    );

    // Configured with the v2 project and nothing else: its datasets, no warning.
    let mut w = McpClient::spawn(&[("VSV2_MCP_BQ_PROJECT", DEFAULT_PROJECT)]).await;
    let r = w.call("visma_init", json!({})).await;
    let cap = data(&r)["capabilities"]["warehouse"].clone();
    let default_list: Vec<&str> = DEFAULT_DATASETS.split(',').collect();
    assert_eq!(cap["project"], DEFAULT_PROJECT);
    assert_eq!(cap["readable_datasets"], json!(default_list));
    assert!(cap.get("warning").is_none(), "{cap}");
    let tools = w.tools().await;
    for name in [
        "warehouse_query",
        "warehouse_list_tables",
        "warehouse_overview",
    ] {
        let d = tools
            .iter()
            .find(|t| t["name"] == name)
            .and_then(|t| t["description"].as_str())
            .unwrap_or_else(|| panic!("{name} missing"));
        assert!(d.contains(DEFAULT_PROJECT), "{name}: {d}");
    }
    let query = tools
        .iter()
        .find(|t| t["name"] == "warehouse_query")
        .unwrap();
    assert!(query["description"]
        .as_str()
        .unwrap()
        .contains(&format!("{DEFAULT_PROJECT}.dataset.table")));

    // Another project is allowed but flagged: its queries would be refused by the default dataset list.
    let mut o = McpClient::spawn(&[("VSV2_MCP_BQ_PROJECT", "some-other-project")]).await;
    let r = o.call("visma_init", json!({})).await;
    let cap = &data(&r)["capabilities"]["warehouse"];
    assert_eq!(cap["project"], "some-other-project");
    let warning = cap["warning"].as_str().expect("a warning");
    assert!(
        warning.contains("some-other-project") && warning.contains(DEFAULT_PROJECT),
        "{warning}"
    );
}

#[tokio::test]
async fn every_tool_carries_its_skill_list_in_description_and_meta() {
    let mut c = McpClient::spawn(&[]).await;
    let tools = c.tools().await;
    assert!(tools.len() >= 5, "local tools expected: {tools:?}");
    for t in &tools {
        let name = t["name"].as_str().unwrap();
        let skills = t["_meta"]["skills"]
            .as_array()
            .unwrap_or_else(|| panic!("{name}: no _meta.skills in {t}"));
        assert!(!skills.is_empty(), "{name}: empty skill list");
        let first = skills[0].as_str().unwrap();
        let desc = t["description"].as_str().unwrap();
        assert!(
            desc.contains(&format!("`{first}`")),
            "{name}: description must name its first skill `{first}`: {desc}"
        );
        if ["visma_init", "visma_list_skills", "visma_get_skill"].contains(&name) {
            // the tools you use to start or to read skills cannot ask you to read a skill first
            assert!(
                desc.contains("without reading them first"),
                "{name}: {desc}"
            );
        } else {
            assert!(
                desc.contains("visma_get_skill"),
                "{name}: description must say how to read the skill: {desc}"
            );
        }
        // only tools that touch something outside this process are "open world"
        let local = [
            "visma_init",
            "visma_list_skills",
            "visma_get_skill",
            "visma_list_endpoints",
            "visma_read_result",
        ];
        assert_eq!(
            t["annotations"]["openWorldHint"] == true,
            !local.contains(&name),
            "{name}: openWorldHint"
        );
    }
}

#[tokio::test]
async fn skills_can_be_listed_by_tool_searched_and_read_over_the_wire() {
    let mut c = McpClient::spawn(&[]).await;

    let by_tool = c
        .call("visma_list_skills", json!({"tool": "visma_erp_query"}))
        .await;
    let d = data(&by_tool);
    assert_eq!(
        d["skills"][0]["id"], "live-erp-queries",
        "the primary skill comes first: {d}"
    );
    assert!(d["skills"].as_array().unwrap().len() >= 3);
    // This hermetic server has no live tools: a skill that needs one says so.
    assert!(
        d["skills"][0]["unavailable_tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t == "visma_erp_query"),
        "a skill must name the tools this instance lacks: {d}"
    );

    let all = c.call("visma_list_skills", json!({})).await;
    assert_eq!(
        data(&all)["count"].as_u64().unwrap() as usize,
        skills_dir_ids().len()
    );

    let tagged = c
        .call("visma_list_skills", json!({"tag": "projects"}))
        .await;
    assert!(data(&tagged)["skills"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["id"] == "project-profitability"));

    let one = c
        .call("visma_get_skill", json!({"id": "data-freshness"}))
        .await;
    let skill = &data(&one)["skill"];
    assert!(
        data(&one)["availability_note"]
            .as_str()
            .unwrap()
            .contains("visma_init"),
        "reading a skill on a server that lacks some of its tools says so"
    );
    let content = skill["content"].as_str().unwrap();
    assert!(
        content.contains("```tool-call") && content.contains("visma_sync_status"),
        "the full body with its worked examples is returned"
    );
    assert!(skill["related_tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t == "warehouse_overview"));

    let found = c
        .call(
            "visma_get_skill",
            json!({"query": "overdue invoices aging"}),
        )
        .await;
    assert_eq!(data(&found)["skill"]["id"], "receivables-payables");
    // models often send "" for a field they do not use, and search whole words ("AR" is not in "datamart")
    let blank = c
        .call("visma_get_skill", json!({"id": "", "query": "AR aging"}))
        .await;
    assert_eq!(data(&blank)["skill"]["id"], "receivables-payables");
    let blank_list = c
        .call("visma_list_skills", json!({"query": "", "tag": ""}))
        .await;
    assert_eq!(
        data(&blank_list)["count"].as_u64().unwrap() as usize,
        skills_dir_ids().len()
    );

    let missing = c
        .call("visma_get_skill", json!({"id": "no-such-skill"}))
        .await;
    // An unknown id is a bad argument (a protocol error) that names near matches and the way out.
    let text = missing["error"]["message"].as_str().unwrap_or_default();
    assert!(
        text.contains("no-such-skill")
            && text.contains("did you mean")
            && text.contains("see skill"),
        "{missing}"
    );
}

#[tokio::test]
async fn skills_are_also_readable_as_resources() {
    let mut c = McpClient::spawn(&[]).await;
    let list = c.request("resources/list", json!({})).await;
    let uris: BTreeSet<String> = list["result"]["resources"]
        .as_array()
        .unwrap_or_else(|| panic!("resources/list failed: {list}"))
        .iter()
        .filter_map(|r| r["uri"].as_str().map(String::from))
        .collect();
    let expected: BTreeSet<String> = skills_dir_ids()
        .into_iter()
        .map(|id| format!("skill://vsv2/{id}"))
        .collect();
    assert_eq!(uris, expected);

    let read = c
        .request(
            "resources/read",
            json!({"uri": "skill://vsv2/sales-reporting"}),
        )
        .await;
    let text = read["result"]["contents"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("resources/read failed: {read}"));
    assert!(text.contains("Sales reporting") && text.contains("behavior"));

    let bad = c
        .request("resources/read", json!({"uri": "skill://vsv2/nope"}))
        .await;
    assert!(
        bad.get("error").is_some(),
        "unknown skill is an error: {bad}"
    );
    let outside = c
        .request("resources/read", json!({"uri": "file:///etc/passwd"}))
        .await;
    assert!(
        outside.get("error").is_some(),
        "only skill:// resources exist: {outside}"
    );
}

#[test]
fn the_manifest_shows_the_whole_surface_without_a_client() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_vsv2-mcp"))
        .arg("--print-manifest")
        .current_dir(dir.path())
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("VSV2_ENDPOINT_CONFIG_DIR", repo_endpoint_dir())
        .output()
        .expect("run vsv2-mcp --print-manifest");
    assert!(
        out.status.success(),
        "manifest failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let m: Value = serde_json::from_slice(&out.stdout).expect("the manifest is one JSON document");

    let skill_ids: BTreeSet<String> = m["skills"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(skill_ids, skills_dir_ids());

    let tools = m["tools"].as_array().unwrap();
    assert!(!tools.is_empty());
    for t in tools {
        let name = t["name"].as_str().unwrap();
        let skills = t["skills"].as_array().unwrap();
        assert!(!skills.is_empty(), "{name} has no skills in the manifest");
        for s in skills {
            assert!(
                skill_ids.contains(s.as_str().unwrap()),
                "{name} lists unknown skill {s}"
            );
        }
    }
    assert!(
        m["prompts"].as_array().unwrap().is_empty(),
        "the manifest shows what THIS instance offers; without tools there are no workflows"
    );
    assert_eq!(m["status"]["capabilities"]["warehouse"]["enabled"], false);
}

#[test]
fn the_manifest_follows_the_configuration_and_a_mistyped_flag_does_not_start_a_server() {
    let dir = tempfile::tempdir().unwrap();
    let run = |args: &[&str], bq: Option<&str>| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_vsv2-mcp"));
        cmd.args(args)
            .current_dir(dir.path())
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("VSV2_ENDPOINT_CONFIG_DIR", repo_endpoint_dir());
        if let Some(p) = bq {
            cmd.env("VSV2_MCP_BQ_PROJECT", p);
        }
        cmd.stdin(std::process::Stdio::null())
            .output()
            .expect("run vsv2-mcp")
    };

    let with_warehouse = run(&["--print-manifest"], Some("some-project"));
    assert!(with_warehouse.status.success());
    let m: Value = serde_json::from_slice(&with_warehouse.stdout).unwrap();
    let names: BTreeSet<&str> = m["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(names.contains("warehouse_query") && !names.contains("visma_salesorder_patch_plan"));
    assert_eq!(m["prompts"].as_array().unwrap().len(), 7);

    // a typo must be an error, not a server that waits on stdin
    let typo = run(&["--print-manifst"], None);
    assert!(!typo.status.success(), "unknown flags must fail");
    let err = String::from_utf8_lossy(&typo.stderr);
    assert!(
        err.contains("unknown argument") && err.contains("usage: vsv2-mcp"),
        "{err}"
    );
    let help = run(&["--help"], None);
    assert!(
        help.status.success() && String::from_utf8_lossy(&help.stdout).contains("--print-manifest")
    );
}
