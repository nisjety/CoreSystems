//! The server's whole surface as one JSON document (`vsv2-mcp --print-manifest`): tools with their
//! input schemas and skill lists, prompts, skills, and the capability status. For review, for
//! diffing between releases, and for tests; no client is needed to see what a client would see.

use serde_json::{json, Value};

use crate::status::ServerStatus;
use crate::tool::{ToolKind, ToolRegistry};

fn kind(k: ToolKind) -> &'static str {
    match k {
        ToolKind::Read => "read",
        ToolKind::Plan => "plan",
        ToolKind::Write => "write",
    }
}

pub fn manifest(registry: &ToolRegistry, status: &ServerStatus) -> Value {
    let tools: Vec<Value> = registry
        .decorated_defs()
        .into_iter()
        .map(|d| {
            json!({
                "name": d.def.name,
                "kind": kind(d.def.kind),
                "description": d.def.description,
                "input_schema": d.def.input_schema,
                "skills": d.skills,
            })
        })
        .collect();
    let names: std::collections::BTreeSet<String> =
        registry.names().iter().map(|n| (*n).to_string()).collect();
    let prompts: Vec<Value> = crate::prompts::available(&names)
        .map(|p| {
            json!({
                "name": p.name,
                "title": p.title,
                "description": p.description,
                "arguments": p.args.iter().map(|a| json!({"name": a.name, "required": a.required, "description": a.description})).collect::<Vec<_>>(),
            })
        })
        .collect();
    let skills: Vec<Value> = registry
        .skills()
        .map(|i| i.all().iter().map(|s| s.summary()).collect())
        .unwrap_or_default();
    json!({
        "server": {"name": "vsv2-mcp", "version": env!("CARGO_PKG_VERSION")},
        "tools": tools,
        "prompts": prompts,
        "skills": skills,
        "status": {"capabilities": status.capabilities_json(), "notes": status.notes},
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;

    #[test]
    fn lists_tools_with_their_skills_prompts_and_skills() {
        let reg = crate::tools::register_init(
            crate::tools::local_tools(Catalog::default()),
            ServerStatus::default(),
        );
        let m = manifest(&reg, &ServerStatus::default());
        let tools = m["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"visma_list_endpoints") && names.contains(&"visma_get_skill"));
        let get = tools
            .iter()
            .find(|t| t["name"] == "visma_get_skill")
            .unwrap();
        assert_eq!(get["kind"], "read");
        assert!(get["skills"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s == "start-here"));
        assert!(get["description"]
            .as_str()
            .unwrap()
            .contains("Skills for this tool"));
        // The manifest shows what this instance offers: no warehouse or write tools here, so no
        // prompt whose workflow needs them.
        let have: std::collections::BTreeSet<String> =
            names.iter().map(|n| (*n).to_string()).collect();
        assert_eq!(
            m["prompts"].as_array().unwrap().len(),
            crate::prompts::available(&have).count()
        );
        assert_eq!(m["prompts"].as_array().unwrap().len(), 0);
        assert!(!m["skills"].as_array().unwrap().is_empty());
    }
}
