//! `vsv2-mcp` — Layer 4: MCP server over the Visma data warehouse and Visma itself.
//!
//! Tools are plain async functions behind the SDK-independent [`Tool`] trait, so they can be
//! unit-tested without a transport. [`server::McpServer`] adapts a [`ToolRegistry`] to the `rmcp`
//! stdio server. Read tools never need write credentials; write tools are only registered when
//! explicitly enabled and still go through Layer 3's dry-run/confirm/audit pipeline.

pub mod catalog;
pub mod config;
pub mod identity;
pub mod known_issues;
pub mod manifest;
pub mod meta;
pub mod prompts;
pub mod reference;
pub mod results;
pub mod server;
pub mod signin_cli;
pub mod skills;
#[cfg(test)]
mod skills_check;
pub mod status;
#[cfg(test)]
pub(crate) mod testutil;
pub mod tool;
pub mod tools;
pub mod visma;
pub mod warehouse;

pub use catalog::{Catalog, CatalogEntry};
pub use config::McpConfig;
pub use server::McpServer;
pub use tool::{
    parse_args, Approver, CallContext, Tool, ToolDef, ToolError, ToolKind, ToolRegistry,
    MAX_OUTPUT_BYTES,
};
