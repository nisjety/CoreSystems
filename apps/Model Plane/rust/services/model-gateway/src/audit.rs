//! Inline-tool-loop `tool_action` audit emitter (E5).
//!
//! The governed agentic path (execution-core → session-core `audit_publisher`)
//! already emits a `velion.audit.v1.model.tool_action` event for every tool
//! step. The *inline* chat tool loop (`features:["tools"]` WITHOUT `agentic`)
//! executes builtin tools directly in model-gateway (`tool_loop`) and
//! historically emitted no audit — so AI tool use on that path was not
//! auditable. This module closes that gap: it publishes the SAME flat audit
//! body audit-core consumes (canonical contract: session-core
//! `audit_publisher::build_tool_action_body` / `SUBJECT_MODEL_TOOL_ACTION`),
//! best-effort, for each inline tool call.

use std::sync::Arc;

use tokio::sync::OnceCell;

/// NATS subject audit-core subscribes (`velion.audit.v1.>`) for model tool
/// calls — mirror of session-core `audit_publisher::SUBJECT_MODEL_TOOL_ACTION`.
const SUBJECT_MODEL_TOOL_ACTION: &str = "velion.audit.v1.model.tool_action";

/// Process-global, lazily-connected NATS client for inline-tool audit emission.
/// `None` when `NATS_URL` is unset or the connection fails — audit is
/// best-effort and must never break a chat turn. Connects at most once.
static AUDIT_CLIENT: OnceCell<Option<Arc<async_nats::Client>>> = OnceCell::const_new();

async fn client() -> Option<Arc<async_nats::Client>> {
    AUDIT_CLIENT
        .get_or_init(|| async {
            let url = std::env::var("NATS_URL").ok().filter(|s| !s.is_empty())?;
            match async_nats::connect(&url).await {
                Ok(c) => {
                    tracing::info!(url = %url, "model-gateway inline-tool audit publisher connected");
                    Some(Arc::new(c))
                }
                Err(error) => {
                    tracing::warn!(%error, "inline-tool audit publisher NATS connect failed; inline tool_action audit disabled");
                    None
                }
            }
        })
        .await
        .clone()
}

/// Best-effort data-category classification for an inline builtin tool. The
/// governed path's per-tool classification (execution-core) is authoritative;
/// this is the inline-path approximation — known public-web/registry tools are
/// `public_non_personal`, the org-knowledge tool is `customer_private`, and
/// anything unrecognized is `unclassified` (never a fabricated category).
#[must_use]
pub fn tool_data_category(tool: &str) -> &'static str {
    match tool {
        "web_search" | "fetch_url" | "web_fetch" | "news" | "traffic" | "yr_weather"
        | "company_lookup" | "track_shipment" => "public_non_personal",
        "knowledge_search" => "customer_private",
        _ => "unclassified",
    }
}

/// Build the FLAT audit body matching audit-core's `AuditEvent` json tags
/// (required by its `Validate()`: `org_id`, `plane`, `event`). Identical shape
/// to session-core's governed-path body so the two emitters are consistent.
// Every arg is a distinct audit field; a parameter struct would only add
// indirection (mirrors session-core's `build_tool_action_body`).
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn build_tool_action_body(
    org_id: &str,
    user_id: &str,
    run_id: &str,
    call_id: &str,
    tool: &str,
    outcome: &str,
    zdr: bool,
    occurred_at: chrono::DateTime<chrono::Utc>,
) -> serde_json::Value {
    serde_json::json!({
        "occurred_at": occurred_at.to_rfc3339(),
        "org_id": org_id,
        // Agentic/inline tool steps run on the user's behalf; fall back to
        // "agent" when the user is unknown (matches the governed path).
        "user_id": if user_id.is_empty() { "agent" } else { user_id },
        "plane": "model",
        "event": "tool_action",
        "subject": run_id,
        "resource_id": call_id,
        "outcome": outcome,
        "details": {
            "tool": tool,
            "data_category": tool_data_category(tool),
            "zdr": zdr,
        },
    })
}

/// Publish a `tool_action` audit for one inline tool call. Best-effort: any
/// failure (no NATS, encode error, publish error) is logged and swallowed — it
/// must never fail the chat turn that triggered the tool.
pub async fn publish_inline_tool_action(
    org_id: &str,
    user_id: &str,
    run_id: &str,
    call_id: &str,
    tool: &str,
    outcome: &str,
    zdr: bool,
) {
    // audit-core's Validate() requires a non-empty org_id; skip silently if absent.
    if org_id.is_empty() {
        return;
    }
    let Some(client) = client().await else {
        return;
    };
    let body = build_tool_action_body(
        org_id,
        user_id,
        run_id,
        call_id,
        tool,
        outcome,
        zdr,
        chrono::Utc::now(),
    );
    let bytes = match serde_json::to_vec(&body) {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::warn!(%error, "inline tool_action audit encode failed");
            return;
        }
    };
    if let Err(error) = client
        .publish(SUBJECT_MODEL_TOOL_ACTION.to_owned(), bytes.into())
        .await
    {
        tracing::warn!(%error, subject = SUBJECT_MODEL_TOOL_ACTION, "inline tool_action audit publish failed (best-effort)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_category_maps_known_tools_and_defaults_unclassified() {
        assert_eq!(tool_data_category("company_lookup"), "public_non_personal");
        assert_eq!(tool_data_category("web_search"), "public_non_personal");
        assert_eq!(tool_data_category("knowledge_search"), "customer_private");
        assert_eq!(tool_data_category("some_mcp_tool"), "unclassified");
    }

    #[test]
    fn body_matches_audit_core_shape() {
        let body = build_tool_action_body(
            "org_1",
            "user_1",
            "req_1",
            "call_abc",
            "company_lookup",
            "ok",
            false,
            chrono::Utc::now(),
        );
        assert_eq!(body["org_id"], "org_1");
        assert_eq!(body["user_id"], "user_1");
        assert_eq!(body["plane"], "model");
        assert_eq!(body["event"], "tool_action");
        assert_eq!(body["subject"], "req_1");
        assert_eq!(body["resource_id"], "call_abc");
        assert_eq!(body["outcome"], "ok");
        assert_eq!(body["details"]["tool"], "company_lookup");
        assert_eq!(body["details"]["data_category"], "public_non_personal");
        assert_eq!(body["details"]["zdr"], false);
        assert!(body["occurred_at"].is_string());
    }

    #[test]
    fn empty_user_falls_back_to_agent() {
        let body = build_tool_action_body(
            "org_1",
            "",
            "req_1",
            "call_1",
            "web_search",
            "ok",
            true,
            chrono::Utc::now(),
        );
        assert_eq!(body["user_id"], "agent");
        assert_eq!(body["details"]["zdr"], true);
    }
}
