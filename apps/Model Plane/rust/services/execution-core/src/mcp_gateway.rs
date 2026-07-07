//! Bridge from the governed agent loop to model-gateway's MCP registry.
//!
//! execution-core has no MCP registry of its own — per the capability-ownership
//! matrix §G2 the **gateway** owns MCP server registration, discovery, and the
//! tool proxy. To let the governed loop OFFER an org's registered MCP tools to
//! the model and ROUTE the calls it makes, exec-core dials the gateway's
//! `ModelGateway` gRPC: `ListMcpTools` for the agent-facing tool defs (namespaced
//! `mcp__<server_id>__<tool>`), and the existing `ProxyMcpTool` to execute one.
//!
//! Both directions are **best-effort**: a missing or unreachable gateway yields
//! no MCP tools, and the agent still runs with its built-in toolset. The gateway
//! shares `model-plane-network` with exec-core, so the default address resolves
//! in-cluster without extra wiring.

use mp_contracts::model_plane::v1 as pb;
use mp_contracts::model_plane::v1::model_gateway_client::ModelGatewayClient;
use tonic::transport::Channel;

/// In-network model-gateway gRPC address (shared `model-plane-network`).
const DEFAULT_ADDR: &str = "http://model-gateway:9090";

/// Lazily-connected client to the gateway's MCP surface.
#[derive(Clone)]
pub struct McpGatewayClient {
    channel: Channel,
}

impl McpGatewayClient {
    /// Build a lazily-connected client. Reads `MODEL_GATEWAY_ADDR` (or
    /// `MODEL_GATEWAY_GRPC_URL`), defaulting to the in-network gateway.
    /// `connect_lazy` never dials here; a dead gateway surfaces at call time and
    /// is handled best-effort. Returns `None` only if the address is unparseable.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let addr = std::env::var("MODEL_GATEWAY_ADDR")
            .or_else(|_| std::env::var("MODEL_GATEWAY_GRPC_URL"))
            .unwrap_or_else(|_| DEFAULT_ADDR.to_owned());
        let channel = tonic::transport::Endpoint::from_shared(addr)
            .ok()?
            .connect_timeout(std::time::Duration::from_secs(2))
            .timeout(std::time::Duration::from_secs(8))
            .connect_lazy();
        Some(Self { channel })
    }

    /// Agent-facing MCP tool defs for `(org_id, user_id)`, namespaced
    /// `mcp__<server>__<tool>` and filtered to what this user may USE (org-wide,
    /// owned, or shared-to-them — never another user's private tools).
    /// Best-effort: any transport/RPC error yields an empty list (offer nothing).
    pub async fn list_tools(&self, org_id: &str, user_id: &str) -> Vec<pb::ToolDefinition> {
        let mut client = ModelGatewayClient::new(self.channel.clone());
        match client
            .list_mcp_tools(pb::ListMcpToolsRequest {
                request_id: String::new(),
                org_id: org_id.to_owned(),
                user_id: user_id.to_owned(),
            })
            .await
        {
            Ok(resp) => resp.into_inner().tools,
            Err(e) => {
                tracing::debug!(error = %e, "mcp list_tools failed; offering no MCP tools");
                Vec::new()
            }
        }
    }

    /// Proxy a single MCP tool call through the gateway. Returns the tool's
    /// output JSON, or an error string the model can see and react to.
    ///
    /// # Errors
    /// Returns `Err` if the gateway gRPC call fails (transport) or the remote
    /// MCP server reports an error (`error_message` non-empty).
    pub async fn proxy_tool(
        &self,
        org_id: &str,
        server_id: &str,
        tool_name: &str,
        input_json: &str,
    ) -> Result<String, String> {
        let mut client = ModelGatewayClient::new(self.channel.clone());
        let resp = client
            .proxy_mcp_tool(pb::ProxyMcpToolRequest {
                request_id: String::new(),
                org_id: org_id.to_owned(),
                server_id: server_id.to_owned(),
                tool_name: tool_name.to_owned(),
                input_json: input_json.to_owned(),
            })
            .await
            .map_err(|e| format!("mcp proxy transport: {e}"))?
            .into_inner();
        if resp.error_message.is_empty() {
            Ok(resp.output_json)
        } else {
            Err(resp.error_message)
        }
    }
}

/// Split `mcp__<server_id>__<tool>` into `(server_id, tool)`. Mirrors the
/// gateway's parse: split on the FIRST `__` after the prefix (a tool name may
/// itself contain `__`; a `server_id` ULID does not).
#[must_use]
pub fn parse_mcp_tool_name(name: &str) -> Option<(&str, &str)> {
    name.strip_prefix("mcp__")
        .and_then(|rest| rest.split_once("__"))
}

#[cfg(test)]
mod tests {
    use super::parse_mcp_tool_name;

    #[test]
    fn parses_server_and_tool() {
        assert_eq!(
            parse_mcp_tool_name("mcp__01ABC__read_file"),
            Some(("01ABC", "read_file"))
        );
    }

    #[test]
    fn splits_on_first_separator_so_tool_may_contain_underscores() {
        assert_eq!(
            parse_mcp_tool_name("mcp__srv__do__a__thing"),
            Some(("srv", "do__a__thing"))
        );
    }

    #[test]
    fn rejects_non_mcp_and_malformed() {
        assert_eq!(parse_mcp_tool_name("web_search"), None);
        assert_eq!(parse_mcp_tool_name("mcp__noseparator"), None);
    }
}
