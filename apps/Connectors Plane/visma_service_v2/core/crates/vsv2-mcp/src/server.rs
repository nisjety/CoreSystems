//! Adapter: exposes a [`ToolRegistry`] as an `rmcp` MCP server.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use rmcp::model::*;
use rmcp::service::{ElicitationError, Peer, RequestContext};
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use serde_json::Value;

use crate::tool::{Approver, CallContext, ToolError, ToolKind, ToolRegistry};

/// How long a person gets to answer an approval prompt.
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);

/// Tools that touch nothing outside this process (local config, the bundled skills, parked results),
/// so they are not "open world" in the MCP sense.
const LOCAL_ONLY: &[&str] = &[
    crate::tools::init::NAME,
    crate::tools::skills::LIST,
    crate::tools::skills::GET,
    crate::tools::endpoints::NAME,
    crate::tool::RESULT_TOOL,
];

/// The form shown to the person: one yes/no.
#[derive(Debug, serde::Serialize, serde::Deserialize, rmcp::schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct Confirm {
    /// Tick to approve this change.
    approve: bool,
}
rmcp::elicit_safe!(Confirm);

/// Asks the person behind the MCP client, through MCP elicitation.
struct PeerApprover(Peer<RoleServer>);

#[async_trait]
impl Approver for PeerApprover {
    async fn approve(&self, message: &str) -> Result<bool, String> {
        match self
            .0
            .elicit_with_timeout::<Confirm>(message.to_string(), Some(APPROVAL_TIMEOUT))
            .await
        {
            Ok(Some(c)) => Ok(c.approve),
            Ok(None) | Err(ElicitationError::UserDeclined | ElicitationError::UserCancelled) => {
                Ok(false)
            }
            Err(ElicitationError::CapabilityNotSupported) => Err(
                "the MCP client does not support elicitation, so the user cannot be asked".into(),
            ),
            Err(other) => Err(format!("approval prompt failed: {other}")),
        }
    }
}

#[derive(Clone)]
pub struct McpServer {
    registry: ToolRegistry,
    instructions: String,
}

impl McpServer {
    pub fn new(registry: ToolRegistry) -> Self {
        Self {
            registry,
            instructions: "Aquatiq Visma data warehouse and Visma ERP. Call visma_init first: it says what \
                this server can do right now, why anything is disabled, and which data issues are known. Every tool has skills (tested \
                usage guides with exact table, column and API conventions): read the skill for a tool \
                before using it for the first time, and always before a write (visma_list_skills with a \
                tool name, then visma_get_skill). Read tools are safe and side-effect free; write tools \
                (if present) only produce a reviewable plan until the user approves the change in a \
                prompt shown by the client."
                .into(),
        }
    }

    pub fn registry(&self) -> &ToolRegistry {
        &self.registry
    }

    /// What the server tells every client at connection time (checked by tests against the tools).
    pub fn instructions(&self) -> &str {
        &self.instructions
    }

    fn tool_names(&self) -> BTreeSet<String> {
        self.registry
            .names()
            .iter()
            .map(|n| (*n).to_string())
            .collect()
    }

    fn to_mcp_tool(d: &crate::tool::DecoratedDef) -> Tool {
        let def = &d.def;
        let schema = match &def.input_schema {
            Value::Object(m) => m.clone(),
            _ => Default::default(),
        };
        let (read_only, destructive) = match def.kind {
            ToolKind::Read => (true, false),
            ToolKind::Plan => (false, false),
            ToolKind::Write => (false, true),
        };
        let mut tool = Tool::new(def.name, def.description.clone(), schema).annotate(
            ToolAnnotations::from_raw(
                None,
                Some(read_only),
                Some(destructive),
                Some(read_only),
                Some(!LOCAL_ONLY.contains(&def.name)),
            ),
        );
        // The tool's own skill list, machine-readable (the description names the first few).
        let mut meta = serde_json::Map::new();
        meta.insert("skills".into(), serde_json::json!(d.skills));
        tool.meta = Some(MetaObject::from(meta));
        tool
    }

    fn skill_uri(id: &str) -> String {
        format!("skill://vsv2/{id}")
    }
}

impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .enable_resources()
                .build(),
        )
        .with_instructions(self.instructions.clone())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let tools = self
            .registry
            .decorated_defs()
            .iter()
            .map(Self::to_mcp_tool)
            .collect();
        Ok(ListToolsResult {
            tools,
            ..Default::default()
        })
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let resources = self
            .registry
            .skills()
            .map(|idx| {
                idx.all()
                    .iter()
                    .map(|s| {
                        Resource::new(Self::skill_uri(&s.id), s.id.clone())
                            .with_title(s.title.clone())
                            .with_description(s.description.clone())
                            .with_mime_type("text/markdown")
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(ListResourcesResult {
            resources,
            ..Default::default()
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        let id = request.uri.strip_prefix("skill://vsv2/").ok_or_else(|| {
            McpError::invalid_params(format!("unknown resource: {}", request.uri), None)
        })?;
        let skill = self
            .registry
            .skills()
            .and_then(|idx| idx.get(id).cloned())
            .ok_or_else(|| McpError::invalid_params(format!("unknown skill: {id}"), None))?;
        let text = format!(
            "# {}\n\n{}\n\n---\n{}",
            skill.title, skill.description, skill.body
        );
        Ok(ReadResourceResult::new(vec![
            ResourceContents::text(text, request.uri.clone()).with_mime_type("text/markdown")
        ])
        .into())
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        let names = self.tool_names();
        let prompts = crate::prompts::available(&names)
            .map(|p| {
                let args: Vec<PromptArgument> = p
                    .args
                    .iter()
                    .map(|a| {
                        PromptArgument::new(a.name)
                            .with_description(a.description)
                            .with_required(a.required)
                    })
                    .collect();
                Prompt::new(
                    p.name,
                    Some(p.description),
                    (!args.is_empty()).then_some(args),
                )
                .with_title(p.title)
            })
            .collect();
        Ok(ListPromptsResult {
            prompts,
            ..Default::default()
        })
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, McpError> {
        let def = crate::prompts::get(&request.name).ok_or_else(|| {
            McpError::invalid_params(format!("unknown prompt: {}", request.name), None)
        })?;
        let missing = crate::prompts::missing_tools(def, &self.tool_names());
        if !missing.is_empty() {
            return Err(McpError::invalid_params(
                format!(
                    "prompt {} needs tools this server instance does not have: {} ({} says why)",
                    def.name,
                    missing.join(", "),
                    crate::tools::init::NAME
                ),
                None,
            ));
        }
        let mut args = std::collections::BTreeMap::new();
        for (k, v) in request.arguments.unwrap_or_default() {
            let text = v.as_str().map(str::to_string).ok_or_else(|| {
                McpError::invalid_params(format!("prompt argument {k:?} must be a string"), None)
            })?;
            args.insert(k, text);
        }
        let text = crate::prompts::render(def.name, &args)
            .map_err(|m| McpError::invalid_params(m, None))?;
        Ok(
            GetPromptResult::new(vec![PromptMessage::new_text(Role::User, text)])
                .with_description(def.description)
                .into(),
        )
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let args = request.arguments.map(Value::Object).unwrap_or(Value::Null);
        let ctx = CallContext::with_approver(Arc::new(PeerApprover(context.peer.clone())));
        match self
            .registry
            .call_with_context(&request.name, args, &ctx)
            .await
        {
            Ok(v) => Ok(CallToolResult::structured(v).into()),
            Err(ToolError::Unknown(n)) => {
                Err(McpError::invalid_params(format!("unknown tool: {n}"), None))
            }
            Err(ToolError::InvalidArgs(m)) => Err(McpError::invalid_params(m, None)),
            // Tool ran but could not do the job: visible to the model as an error result.
            Err(ToolError::Failed(m)) => {
                Ok(CallToolResult::error(vec![ContentBlock::text(m)]).into())
            }
        }
    }
}
