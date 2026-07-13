//! §4.3 capability-registry cache-coherence consumer — the READ-path dual of
//! the H.1 MCP write-through.
//!
//! capability-core is the registry system-of-record; the gateway's
//! [`McpRegistry`] is a cache (matrix §4.1/H.1). H.1 wired the WRITE path
//! (gateway `register_mcp_server` → capability-core `POST /api/v1/mcp`). This is
//! the READ path: subscribe to capability-core's reconcile events on
//! `mp.v1.capability.mcp_server.*` (published as the shared [`Envelope`]) and
//! keep the cache coherent.
//!
//! ## Scope: removal-coherence only — deliberately, "the right tool for the job"
//!
//! On `mcp_server.removed` the gateway drops the cached entry so it stops
//! proxying to a decommissioned/revoked server (the security-critical
//! staleness). A `registered`/`updated` event is **intentionally ignored**: the
//! bearer `token` is an operational secret the catalog does NOT store (H.1), so
//! the cache cannot be safely reconstructed from a catalog event without caching
//! a tokenless, broken entry — and the gateway that registered the server
//! already holds it. Cross-instance refresh needs a separate, secure
//! token-conveyance design and is out of scope here (recorded in matrix §4.3).
//!
//! ## Verification
//!
//! [`reconcile_action`] (pure) and the cache removal via [`apply`] are
//! unit-tested below. The live subscribe loop ([`run`]) needs a running NATS bus
//! to exercise end-to-end; it is best-effort and self-guarding, so it never
//! breaks the gateway when NATS is absent. (Flagged — matrix §4.3.)

use crate::runtime_registries::McpRegistry;
use mp_events::envelope::Envelope;
use tracing::{info, warn};

/// NATS subject filter for capability reconcile events (`>` = all kinds/actions).
pub const CAPABILITY_SUBJECT_FILTER: &str = "mp.v1.capability.>";

/// `event_type` prefix capability-core stamps on MCP reconcile envelopes.
const MCP_EVENT_PREFIX: &str = "capability.mcp_server.";
/// `resource_ref` prefix for an MCP server (`mcp_server:<id>`).
const RESOURCE_PREFIX: &str = "mcp_server:";

/// The cache mutation a reconcile envelope implies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheAction {
    /// A server was removed from the system-of-record — drop it from the cache.
    Remove { org_id: String, server_id: String },
    /// Nothing to do: a non-MCP event, a non-removal action we deliberately
    /// skip (see module docs), or a malformed reference.
    Ignore,
}

/// Pure: decide the cache action implied by a reconcile [`Envelope`].
///
/// Only `capability.mcp_server.removed` is actioned; the server id is taken
/// from `resource_ref` (`mcp_server:<id>`) and the org from `org_id`. Every
/// other event — other kinds, `registered`/`updated`, empty org, or a malformed
/// `resource_ref` — is [`CacheAction::Ignore`].
#[must_use]
pub fn reconcile_action(env: &Envelope) -> CacheAction {
    let Some(action) = env.event_type.strip_prefix(MCP_EVENT_PREFIX) else {
        return CacheAction::Ignore;
    };
    if action != "removed" || env.org_id.is_empty() {
        return CacheAction::Ignore;
    }
    match env.resource_ref.strip_prefix(RESOURCE_PREFIX) {
        Some(id) if !id.is_empty() => CacheAction::Remove {
            org_id: env.org_id.clone(),
            server_id: id.to_owned(),
        },
        _ => CacheAction::Ignore,
    }
}

/// Apply a decided action to the cache. Touches only the registry.
pub fn apply(action: &CacheAction, mcp: &McpRegistry) {
    if let CacheAction::Remove { org_id, server_id } = action {
        let removed = mcp.remove(org_id, server_id);
        info!(
            org_id = %org_id,
            server_id = %server_id,
            removed,
            "reconcile: MCP server removal applied to gateway cache"
        );
    }
}

/// Run the consumer: connect a NATS subscriber from `NATS_URL`, subscribe to
/// [`CAPABILITY_SUBJECT_FILTER`], and apply each reconcile event to `mcp`.
///
/// Best-effort and self-contained: if `NATS_URL` is unset or the bus is
/// unreachable it logs and returns, never breaking the gateway. Spawn once at
/// startup. capability-core publishes via core NATS (not `JetStream`), so a core
/// subscription with at-most-once delivery is the right fit — a missed removal
/// is corrected by the next event or a cache TTL, and never causes a write.
pub async fn run(mcp: McpRegistry) {
    use futures::StreamExt as _;
    let Ok(url) = std::env::var("NATS_URL") else {
        info!("NATS_URL unset; capability reconcile consumer disabled");
        return;
    };
    let client = match crate::nats_connection::connect(&url).await {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "capability consumer: NATS connect failed; disabled");
            return;
        }
    };
    let mut sub = match client.subscribe(CAPABILITY_SUBJECT_FILTER.to_owned()).await {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "capability consumer: subscribe failed; disabled");
            return;
        }
    };
    info!(
        subject = CAPABILITY_SUBJECT_FILTER,
        "capability reconcile consumer started"
    );
    while let Some(msg) = sub.next().await {
        match serde_json::from_slice::<Envelope>(&msg.payload) {
            Ok(env) => apply(&reconcile_action(&env), &mcp),
            Err(e) => {
                warn!(error = %e, subject = %msg.subject, "capability consumer: undecodable envelope");
            }
        }
    }
    info!("capability reconcile consumer stopped (subscription closed)");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_registries::{handle_register_mcp_server, McpRegistry};
    use mp_contracts::model_plane::v1::{McpServer, RegisterMcpServerRequest};

    // Build an Envelope the same way the wire path does — by deserializing JSON
    // shaped exactly like capability-core's published envelope. This also
    // exercises the shared mp_events::Envelope decode the consumer relies on.
    fn env(event_type: &str, resource_ref: &str, org: &str) -> Envelope {
        let raw = format!(
            r#"{{"event_id":"e1","event_type":"{event_type}","schema_version":1,"ts":"2026-05-30T00:00:00Z","producer":"capability-core","correlation_id":"","causation_id":"","idempotency_key":"","org_id":"{org}","user_id":"","resource_ref":"{resource_ref}","payload":{{}},"zdr":false}}"#
        );
        serde_json::from_str(&raw).expect("valid envelope json")
    }

    #[test]
    fn removed_event_yields_remove_action() {
        let a = reconcile_action(&env(
            "capability.mcp_server.removed",
            "mcp_server:srv-1",
            "org-9",
        ));
        assert_eq!(
            a,
            CacheAction::Remove {
                org_id: "org-9".to_owned(),
                server_id: "srv-1".to_owned()
            }
        );
    }

    #[test]
    fn register_and_update_are_intentionally_ignored() {
        // Cannot reconstruct the token from the catalog -> must not cache.
        for action in ["registered", "updated"] {
            let a = reconcile_action(&env(
                &format!("capability.mcp_server.{action}"),
                "mcp_server:srv-1",
                "org-9",
            ));
            assert_eq!(a, CacheAction::Ignore, "{action} must be ignored");
        }
    }

    #[test]
    fn other_kinds_and_malformed_are_ignored() {
        // Different capability kind.
        assert_eq!(
            reconcile_action(&env("capability.skill.removed", "skill:s1", "o")),
            CacheAction::Ignore
        );
        // Empty org.
        assert_eq!(
            reconcile_action(&env("capability.mcp_server.removed", "mcp_server:s1", "")),
            CacheAction::Ignore
        );
        // Malformed resource_ref (missing id).
        assert_eq!(
            reconcile_action(&env("capability.mcp_server.removed", "mcp_server:", "o")),
            CacheAction::Ignore
        );
        // Non-capability event entirely.
        assert_eq!(
            reconcile_action(&env("session.run.completed", "run:r1", "o")),
            CacheAction::Ignore
        );
    }

    #[test]
    fn apply_remove_drops_the_cached_server() {
        let mcp = McpRegistry::new();
        let req = RegisterMcpServerRequest {
            request_id: "r".to_owned(),
            org_id: "o".to_owned(),
            server: Some(McpServer {
                server_id: "srv-1".to_owned(),
                name: "fs".to_owned(),
                url: "https://mcp.example.test".to_owned(),
                transport: "http".to_owned(),
                token: String::new(),
                tool_allowlist: vec!["read_file".to_owned()],
                enabled: true,
            }),
        };
        handle_register_mcp_server(&mcp, req).expect("register");
        assert!(mcp.contains("o", "srv-1"), "precondition: server is cached");

        apply(
            &reconcile_action(&env(
                "capability.mcp_server.removed",
                "mcp_server:srv-1",
                "o",
            )),
            &mcp,
        );
        assert!(
            !mcp.contains("o", "srv-1"),
            "removal event must drop the cached server"
        );
    }

    #[test]
    fn apply_ignore_is_a_noop() {
        let mcp = McpRegistry::new();
        // Nothing cached; applying Ignore must not panic or change anything.
        apply(&CacheAction::Ignore, &mcp);
        assert!(!mcp.contains("o", "srv-1"));
    }
}
