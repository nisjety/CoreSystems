use async_trait::async_trait;
use serde_json::{json, Value};

use crate::status::ServerStatus;
use crate::tool::{parse_args, Tool, ToolDef, ToolError, ToolKind, MAX_OUTPUT_BYTES};

pub const NAME: &str = "visma_init";

/// "Start here": what this server can do right now, whose authority it uses, and what to read first.
/// A tool that is missing is explained here instead of being a silent absence.
pub struct Init {
    pub status: ServerStatus,
    pub tools: Vec<String>,
    pub prompts: Vec<String>,
}

#[async_trait]
impl Tool for Init {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: NAME,
            description: "Call this first. Returns what this server instance can do right now (which data sources, \
                tenants and write operations are enabled, and why anything is disabled), whose authority it runs \
                with, its limits, the known data issues, and which skills to read first. Local only; no network."
                .into(),
            input_schema: json!({"type": "object", "properties": {}, "additionalProperties": false}),
            kind: ToolKind::Read,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let m: serde_json::Map<String, Value> = parse_args(args)?;
        if !m.is_empty() {
            return Err(ToolError::InvalidArgs(
                "this tool takes no arguments".into(),
            ));
        }
        let skills = crate::skills::SkillIndex::embedded();
        Ok(json!({
            "server": {"name": "vsv2-mcp", "version": env!("CARGO_PKG_VERSION")},
            "acts_as": self.status.acts_as(),
            "start_here": ["start-here"],
            "capabilities": self.status.capabilities_json(),
            "tools": self.tools,
            "prompts": self.prompts,
            "skills": {"count": skills.all().len(), "tags": skills.tags()},
            "known_issues": crate::known_issues::as_json(),
            "limits": {
                "max_tool_output_bytes": MAX_OUTPUT_BYTES,
                "larger_results": "parked behind a result_id; page with visma_read_result",
            },
            "ground_rules": [
                "Read the skill for a tool before using it for the first time (visma_get_skill), and always before a write.",
                "One page is never the whole collection; an empty or failed read is unknown, not zero.",
                "State the tenant, period, currency and data source of every figure you report.",
                "Never change anything in Visma unless the user explicitly asks for that specific change.",
                "Check known_issues against the question: if an issue affects the figures you are about to report, read its skill and say so.",
            ],
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool() -> Init {
        let mut s = ServerStatus::default();
        s.enabled("warehouse", json!({"project": "p"}));
        s.disabled("writes", "not enabled");
        Init {
            status: s,
            tools: vec!["visma_init".into(), "warehouse_query".into()],
            prompts: vec!["financial_overview".into()],
        }
    }

    #[tokio::test]
    async fn explains_capabilities_identity_and_where_to_start() {
        let out = tool().call(json!({})).await.unwrap();
        assert_eq!(out["capabilities"]["warehouse"]["enabled"], true);
        assert_eq!(out["capabilities"]["writes"]["reason"], "not enabled");
        assert!(out["acts_as"]
            .as_str()
            .unwrap()
            .contains("service credentials"));
        assert_eq!(out["start_here"][0], "start-here");
        assert!(out["skills"]["count"].as_u64().unwrap() >= 1);
        assert_eq!(out["server"]["name"], "vsv2-mcp");
        assert!(out["ground_rules"].as_array().unwrap().len() >= 3);
        let issues = out["known_issues"]["issues"].as_array().unwrap();
        assert!(
            !issues.is_empty()
                && issues
                    .iter()
                    .all(|i| i["skill"].is_string() && i["found"].is_string())
        );
    }

    #[tokio::test]
    async fn takes_no_arguments() {
        assert!(tool().call(json!({"x": 1})).await.is_err());
    }
}
