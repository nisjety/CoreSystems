use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::Value;
use thiserror::Error;

/// Hard cap on one tool result. A bigger answer is refused rather than silently truncated, so the
/// model is told to narrow its request instead of reasoning over a cut-off list.
pub const MAX_OUTPUT_BYTES: usize = 200_000;

/// Name of the tool that pages through parked results.
pub const RESULT_TOOL: &str = "visma_read_result";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    /// Cannot change anything anywhere.
    Read,
    /// Prepares a change and records it in the audit log, but changes nothing in Visma.
    Plan,
    /// Can change Visma data (always via Layer 3 dry-run -> confirm -> execute).
    Write,
}

#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: &'static str,
    pub description: String,
    /// JSON Schema (`type: object`) for the arguments.
    pub input_schema: Value,
    pub kind: ToolKind,
}

#[derive(Debug, Error)]
pub enum ToolError {
    /// The caller sent bad arguments (a protocol-level problem the model can fix).
    #[error("invalid arguments: {0}")]
    InvalidArgs(String),
    /// The arguments were fine but the work failed (upstream error, refused by a guardrail, ...).
    #[error("{0}")]
    Failed(String),
    #[error("unknown tool: {0}")]
    Unknown(String),
}

/// Asks a *human* (never the model) to approve something, through the MCP client's own UI.
#[async_trait]
pub trait Approver: Send + Sync {
    /// `Ok(true)` only if the person explicitly said yes. `Err` means the question could not be
    /// asked (e.g. the client has no elicitation support); callers must treat that as "no".
    async fn approve(&self, message: &str) -> Result<bool, String>;
}

/// What the transport can offer a tool for one call.
#[derive(Clone, Default)]
pub struct CallContext {
    pub approver: Option<Arc<dyn Approver>>,
}

impl CallContext {
    /// No way to ask a human.
    pub fn none() -> Self {
        Self::default()
    }

    pub fn with_approver(approver: Arc<dyn Approver>) -> Self {
        Self {
            approver: Some(approver),
        }
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn def(&self) -> ToolDef;
    async fn call(&self, args: Value) -> Result<Value, ToolError>;

    /// Tools that need a human in the loop override this; everything else ignores the context.
    async fn call_with_context(&self, args: Value, _ctx: &CallContext) -> Result<Value, ToolError> {
        self.call(args).await
    }
}

/// Deserialize arguments, rejecting unknown keys (declare `deny_unknown_fields` on `T`).
pub fn parse_args<T: DeserializeOwned>(args: Value) -> Result<T, ToolError> {
    // MCP clients may omit `arguments` entirely for tools with no required input.
    let args = if args.is_null() {
        Value::Object(Default::default())
    } else {
        args
    };
    serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs(e.to_string()))
}

#[derive(Clone, Default)]
pub struct ToolRegistry {
    tools: BTreeMap<&'static str, Arc<dyn Tool>>,
    results: Arc<crate::results::ResultStore>,
    skills: Option<Arc<crate::skills::SkillIndex>>,
}

/// A tool definition as the model should see it: the description names the tool's skills.
#[derive(Debug, Clone)]
pub struct DecoratedDef {
    pub def: ToolDef,
    /// Skill ids for this tool, primary first.
    pub skills: Vec<String>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, tool: impl Tool + 'static) -> Self {
        self.tools.insert(tool.def().name, Arc::new(tool));
        self
    }

    /// Attach the skill index: every tool then lists its skills, and errors point at the first one.
    pub fn with_skills(mut self, skills: Arc<crate::skills::SkillIndex>) -> Self {
        self.skills = Some(skills);
        self
    }

    pub fn skills(&self) -> Option<Arc<crate::skills::SkillIndex>> {
        self.skills.clone()
    }

    pub fn defs(&self) -> Vec<ToolDef> {
        self.tools.values().map(|t| t.def()).collect()
    }

    /// Definitions with each tool's skill list appended to its description.
    pub fn decorated_defs(&self) -> Vec<DecoratedDef> {
        self.defs()
            .into_iter()
            .map(|mut def| {
                let skills: Vec<String> = self
                    .skills
                    .as_ref()
                    .map(|idx| {
                        idx.for_tool(def.name)
                            .iter()
                            .map(|s| s.id.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                def.description
                    .push_str(&crate::skills::description_suffix(def.name, &skills));
                DecoratedDef { def, skills }
            })
            .collect()
    }

    /// ` (see skill `x`: ...)` for the tool's first skill, or nothing.
    fn skill_hint(&self, tool: &str) -> String {
        let first = self
            .skills
            .as_ref()
            .and_then(|idx| idx.for_tool(tool).first().map(|s| s.id.clone()));
        match first {
            Some(id) => format!(" (see skill `{id}`: {} with id)", crate::tools::skills::GET),
            None => String::new(),
        }
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.tools.keys().copied().collect()
    }

    /// Run a tool and enforce the output cap (no human-approval channel available).
    pub async fn call(&self, name: &str, args: Value) -> Result<Value, ToolError> {
        self.call_with_context(name, args, &CallContext::none())
            .await
    }

    pub async fn call_with_context(
        &self,
        name: &str,
        args: Value,
        ctx: &CallContext,
    ) -> Result<Value, ToolError> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| ToolError::Unknown(name.to_string()))?;
        let out = match tool.call_with_context(args, ctx).await {
            Ok(v) => v,
            // Point the model at the right skill when it misuses a tool or a guardrail refuses it.
            Err(ToolError::InvalidArgs(m)) => {
                return Err(ToolError::InvalidArgs(format!(
                    "{m}{}",
                    self.skill_hint(name)
                )))
            }
            Err(ToolError::Failed(m)) => {
                return Err(ToolError::Failed(format!("{m}{}", self.skill_hint(name))))
            }
            Err(other) => return Err(other),
        };
        let size = serde_json::to_vec(&out).map(|v| v.len()).unwrap_or(0);
        if size <= MAX_OUTPUT_BYTES {
            return Ok(out);
        }
        // Too big for the model's context. The paging tool bounds its own pages, so an oversized
        // answer from it is a bug, never something to park again.
        if name == RESULT_TOOL || size > crate::results::MAX_ENTRY_BYTES {
            return Err(ToolError::Failed(format!(
                "result is {size} bytes, over the {MAX_OUTPUT_BYTES}-byte limit and too large to hold for paging; \
                 narrow the request (fewer rows/columns, a filter, a smaller page)"
            )));
        }
        let shape = crate::results::shape(&out);
        let result_id = self.results.put(out);
        Ok(serde_json::json!({
            "result_truncated": true,
            "result_id": result_id,
            "size_bytes": size,
            "expires_in_secs": self.results.ttl_secs(),
            "shape": shape,
            "hint": format!(
                "The full answer is too large to return at once. Page through it with {RESULT_TOOL} \
                 (result_id, a JSON pointer such as /rows, offset, limit), or narrow the original request."
            ),
        }))
    }

    /// The store behind this registry's result handles (for the paging tool).
    pub fn results(&self) -> Arc<crate::results::ResultStore> {
        self.results.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::json;

    struct Echo;
    #[async_trait]
    impl Tool for Echo {
        fn def(&self) -> ToolDef {
            ToolDef {
                name: "echo",
                description: "echo".into(),
                input_schema: json!({"type":"object"}),
                kind: ToolKind::Read,
            }
        }
        async fn call(&self, args: Value) -> Result<Value, ToolError> {
            Ok(args)
        }
    }

    struct Big;
    #[async_trait]
    impl Tool for Big {
        fn def(&self) -> ToolDef {
            ToolDef {
                name: "big",
                description: "".into(),
                input_schema: json!({"type":"object"}),
                kind: ToolKind::Read,
            }
        }
        async fn call(&self, _: Value) -> Result<Value, ToolError> {
            Ok(json!({"x": "a".repeat(MAX_OUTPUT_BYTES + 1)}))
        }
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct A {
        #[allow(dead_code)]
        n: u32,
    }

    #[tokio::test]
    async fn dispatches_by_name_and_rejects_unknown_tools() {
        let r = ToolRegistry::new().with(Echo);
        assert_eq!(
            r.call("echo", json!({"a":1})).await.unwrap(),
            json!({"a":1})
        );
        assert!(matches!(
            r.call("nope", json!({})).await,
            Err(ToolError::Unknown(_))
        ));
        assert_eq!(r.names(), vec!["echo"]);
    }

    #[tokio::test]
    async fn oversized_results_become_a_handle_with_a_shape_and_nothing_is_cut() {
        let r = ToolRegistry::new().with(Big);
        let out = r.call("big", json!({})).await.unwrap();
        assert_eq!(out["result_truncated"], true);
        let id = out["result_id"].as_str().unwrap();
        assert!(out["size_bytes"].as_u64().unwrap() > MAX_OUTPUT_BYTES as u64);
        assert_eq!(out["shape"]["fields"]["x"]["type"], "string");
        assert!(out["hint"].as_str().unwrap().contains(RESULT_TOOL));
        // the full value is still there, untouched
        let held = r.results().get(id).unwrap();
        assert_eq!(held["x"].as_str().unwrap().len(), MAX_OUTPUT_BYTES + 1);
    }

    struct Huge;
    #[async_trait]
    impl Tool for Huge {
        fn def(&self) -> ToolDef {
            ToolDef {
                name: "huge",
                description: "".into(),
                input_schema: json!({"type":"object"}),
                kind: ToolKind::Read,
            }
        }
        async fn call(&self, _: Value) -> Result<Value, ToolError> {
            Ok(json!({"x": "a".repeat(crate::results::MAX_ENTRY_BYTES + 1)}))
        }
    }

    #[tokio::test]
    async fn results_too_big_even_to_hold_are_refused_with_guidance() {
        let r = ToolRegistry::new().with(Huge);
        let err = r.call("huge", json!({})).await.unwrap_err().to_string();
        assert!(err.contains("narrow the request"), "{err}");
    }

    #[test]
    fn parse_args_rejects_unknown_keys_missing_keys_and_accepts_null_as_empty() {
        assert!(parse_args::<A>(json!({"n": 1})).is_ok());
        assert!(matches!(
            parse_args::<A>(json!({"n": 1, "x": 2})),
            Err(ToolError::InvalidArgs(_))
        ));
        assert!(matches!(
            parse_args::<A>(json!({})),
            Err(ToolError::InvalidArgs(_))
        ));
        assert!(parse_args::<serde_json::Map<String, Value>>(Value::Null).is_ok());
    }

    /// Fails in the way the argument says, so a test can reach every error branch.
    struct Flaky;
    #[async_trait]
    impl Tool for Flaky {
        fn def(&self) -> ToolDef {
            ToolDef {
                name: "flaky",
                description: "flaky".into(),
                input_schema: json!({"type":"object"}),
                kind: ToolKind::Read,
            }
        }
        async fn call(&self, args: Value) -> Result<Value, ToolError> {
            match args["how"].as_str() {
                Some("invalid") => Err(ToolError::InvalidArgs("bad input".into())),
                Some("failed") => Err(ToolError::Failed("upstream said no".into())),
                _ => Ok(json!({"ok": true})),
            }
        }
    }

    #[tokio::test]
    async fn both_kinds_of_error_point_at_the_tools_first_skill_but_only_when_it_has_one() {
        let skill = "---\nid: flaky-skill\ntitle: Flaky\ndescription: about flaky\ntags: [x]\ntools: [flaky]\nprimary: [flaky]\n---\nbody";
        let idx = Arc::new(crate::skills::SkillIndex::parse(&[("flaky-skill.md", skill)]).unwrap());
        let reg = ToolRegistry::new().with_skills(idx).with(Flaky).with(Echo);
        let hint = "(see skill `flaky-skill`: visma_get_skill with id)";

        let invalid = reg
            .call("flaky", json!({"how": "invalid"}))
            .await
            .unwrap_err();
        assert!(
            matches!(&invalid, ToolError::InvalidArgs(m) if m.starts_with("bad input") && m.contains(hint)),
            "{invalid:?}"
        );
        let failed = reg
            .call("flaky", json!({"how": "failed"}))
            .await
            .unwrap_err();
        assert!(
            matches!(&failed, ToolError::Failed(m) if m.starts_with("upstream said no") && m.contains(hint)),
            "{failed:?}"
        );

        // no skill for the tool: the message is left alone; unknown tools are not decorated either
        let bad = reg.call("echo", json!({"x": 1})).await.unwrap();
        assert_eq!(bad["x"], 1);
        assert!(matches!(
            reg.call("nope", json!({})).await,
            Err(ToolError::Unknown(_))
        ));
        assert_eq!(reg.skill_hint("echo"), "");
        assert!(reg.skill_hint("flaky").contains("flaky-skill"));
    }
}
