//! Chat-parity rich SSE event taxonomy (docs/chat-parity-audit.md §2).
//!
//! ONE forward-compatible event set for `/v1/invoke/stream`. Each [`ChatEvent`]
//! maps to an SSE `event:` name + JSON `data:` payload the velionv2 client
//! consumes. Rich events are **gated** by the request's opt-in `features[]`, so
//! `profile:"chat"` (empty features) keeps emitting only
//! connected/chunk/done/error — the plain path is untouched and the BFF
//! re-streams unknown events verbatim.
//!
//! This is the one seam every later capability emits into: reasoning, tools,
//! citations, artifacts, steps, usage. Build it once; never duplicate per-feature
//! SSE plumbing.

use std::convert::Infallible;

use axum::response::sse::Event;
use serde_json::{json, Value};
use tokio::sync::mpsc::Sender;

/// A rich chat-stream event beyond the plain text delta.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatEvent {
    /// Collapsible "thinking…" trace tokens.
    ReasoningDelta { delta: String },
    /// Agent-activity timeline / Steps tab.
    StepUpdate {
        id: String,
        title: String,
        detail: String,
        status: String,
    },
    /// A tool/function call the model initiated.
    ToolCall {
        id: String,
        name: String,
        args: Value,
    },
    /// The result of a tool call.
    ToolResult {
        id: String,
        status: String,
        output: String,
        error: Option<String>,
    },
    /// A web/RAG source citation (Sources tab).
    Citation {
        id: String,
        title: String,
        url: String,
        snippet: String,
    },
    /// A canvas/doc/image artifact (side panel).
    Artifact {
        id: String,
        kind: String,
        title: String,
        content: String,
        version: u32,
    },
    /// A generated file/image attachment.
    Attachment {
        id: String,
        name: String,
        mime: String,
        url: String,
        size: i64,
    },
    /// Usage/latency/cost/confidence for the insight chip + reasoning popover.
    Usage {
        input_tokens: u32,
        output_tokens: u32,
        cost_usd: Option<f64>,
        latency_ms: u64,
        confidence: Option<f64>,
    },
    /// Terminal control: generation stopped/cancelled by the user.
    Stopped { reason: String },
}

impl ChatEvent {
    /// The opt-in feature family this event belongs to, or `None` if it is a
    /// control event that is ALWAYS emitted (never gated).
    #[must_use]
    pub fn family(&self) -> Option<&'static str> {
        match self {
            ChatEvent::ReasoningDelta { .. } => Some("reasoning"),
            ChatEvent::StepUpdate { .. } => Some("steps"),
            ChatEvent::ToolCall { .. } | ChatEvent::ToolResult { .. } => Some("tools"),
            ChatEvent::Citation { .. } => Some("citations"),
            ChatEvent::Artifact { .. } | ChatEvent::Attachment { .. } => Some("artifacts"),
            ChatEvent::Usage { .. } => Some("usage"),
            ChatEvent::Stopped { .. } => None, // terminal control — always allowed
        }
    }

    /// Whether to emit this event given the client's opt-in families. Control
    /// events (family `None`) always emit; rich events only when opted in. This
    /// is what protects the plain `chat` path: with no features, only control
    /// events pass.
    #[must_use]
    pub fn should_emit(&self, features: &[String]) -> bool {
        match self.family() {
            None => true,
            Some(fam) => features.iter().any(|f| f == fam),
        }
    }

    /// The SSE `event:` name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            ChatEvent::ReasoningDelta { .. } => "reasoning_delta",
            ChatEvent::StepUpdate { .. } => "step_update",
            ChatEvent::ToolCall { .. } => "tool_call",
            ChatEvent::ToolResult { .. } => "tool_result",
            ChatEvent::Citation { .. } => "citation",
            ChatEvent::Artifact { .. } => "artifact",
            ChatEvent::Attachment { .. } => "attachment",
            ChatEvent::Usage { .. } => "usage",
            ChatEvent::Stopped { .. } => "stopped",
        }
    }

    /// The SSE `data:` JSON payload.
    #[must_use]
    pub fn payload(&self, request_id: &str) -> Value {
        match self {
            ChatEvent::ReasoningDelta { delta } => {
                json!({ "delta": delta, "request_id": request_id })
            }
            ChatEvent::StepUpdate {
                id,
                title,
                detail,
                status,
            } => {
                json!({ "id": id, "title": title, "detail": detail, "status": status })
            }
            ChatEvent::ToolCall { id, name, args } => {
                json!({ "id": id, "name": name, "args": args })
            }
            ChatEvent::ToolResult {
                id,
                status,
                output,
                error,
            } => {
                json!({ "id": id, "status": status, "output": output, "error": error })
            }
            ChatEvent::Citation {
                id,
                title,
                url,
                snippet,
            } => {
                json!({ "id": id, "title": title, "url": url, "snippet": snippet })
            }
            ChatEvent::Artifact {
                id,
                kind,
                title,
                content,
                version,
            } => {
                json!({ "id": id, "kind": kind, "title": title, "content": content, "version": version })
            }
            ChatEvent::Attachment {
                id,
                name,
                mime,
                url,
                size,
            } => {
                json!({ "id": id, "name": name, "type": mime, "url": url, "size": size })
            }
            ChatEvent::Usage {
                input_tokens,
                output_tokens,
                cost_usd,
                latency_ms,
                confidence,
            } => json!({
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
                "cost_usd": cost_usd,
                "latency_ms": latency_ms,
                "confidence": confidence,
            }),
            ChatEvent::Stopped { reason } => json!({ "reason": reason, "request_id": request_id }),
        }
    }

    /// Encode as an axum SSE [`Event`] (name + JSON data).
    pub fn to_sse(&self, request_id: &str) -> Event {
        Event::default()
            .event(self.name())
            .data(self.payload(request_id).to_string())
    }
}

/// Emits [`ChatEvent`]s onto an SSE channel, honoring the client's opt-in
/// `features[]`. Cheap to clone the inputs; holds the sender + gating context.
pub struct RichEventSink {
    tx: Sender<Result<Event, Infallible>>,
    features: Vec<String>,
    request_id: String,
}

impl RichEventSink {
    #[must_use]
    pub fn new(
        tx: Sender<Result<Event, Infallible>>,
        features: Vec<String>,
        request_id: String,
    ) -> Self {
        Self {
            tx,
            features,
            request_id,
        }
    }

    /// Emit an event if the client opted into its family (control events always
    /// emit). Best-effort: a closed channel (client gone) is ignored.
    pub async fn emit(&self, event: ChatEvent) {
        if !event.should_emit(&self.features) {
            return;
        }
        let _ = self.tx.send(Ok(event.to_sse(&self.request_id))).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn families(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn plain_path_emits_only_control_events() {
        // Empty features = profile:"chat" plain path. Rich events are suppressed;
        // the terminal control event (Stopped) still passes.
        let no_features: Vec<String> = vec![];
        assert!(!ChatEvent::ReasoningDelta { delta: "x".into() }.should_emit(&no_features));
        assert!(!ChatEvent::ToolCall {
            id: "1".into(),
            name: "shell".into(),
            args: json!({})
        }
        .should_emit(&no_features));
        assert!(!ChatEvent::Usage {
            input_tokens: 1,
            output_tokens: 2,
            cost_usd: None,
            latency_ms: 10,
            confidence: None,
        }
        .should_emit(&no_features));
        assert!(ChatEvent::Stopped {
            reason: "user".into()
        }
        .should_emit(&no_features));
    }

    #[test]
    fn opt_in_unlocks_only_the_requested_families() {
        let feats = families(&["reasoning", "usage"]);
        assert!(ChatEvent::ReasoningDelta {
            delta: "thinking".into()
        }
        .should_emit(&feats));
        assert!(ChatEvent::Usage {
            input_tokens: 1,
            output_tokens: 2,
            cost_usd: Some(0.01),
            latency_ms: 5,
            confidence: Some(0.9),
        }
        .should_emit(&feats));
        // Not opted in:
        assert!(!ChatEvent::Citation {
            id: "c".into(),
            title: "t".into(),
            url: "u".into(),
            snippet: "s".into(),
        }
        .should_emit(&feats));
        assert!(!ChatEvent::StepUpdate {
            id: "s".into(),
            title: "t".into(),
            detail: "d".into(),
            status: "active".into(),
        }
        .should_emit(&feats));
    }

    #[test]
    fn tools_family_covers_both_call_and_result() {
        let feats = families(&["tools"]);
        assert!(ChatEvent::ToolCall {
            id: "1".into(),
            name: "py".into(),
            args: json!({"a":1})
        }
        .should_emit(&feats));
        assert!(ChatEvent::ToolResult {
            id: "1".into(),
            status: "ok".into(),
            output: "42".into(),
            error: None,
        }
        .should_emit(&feats));
    }

    #[test]
    fn names_and_payload_shapes_are_stable() {
        let usage = ChatEvent::Usage {
            input_tokens: 12,
            output_tokens: 34,
            cost_usd: Some(0.002),
            latency_ms: 880,
            confidence: Some(0.77),
        };
        assert_eq!(usage.name(), "usage");
        let p = usage.payload("req-1");
        assert_eq!(p["input_tokens"], 12);
        assert_eq!(p["output_tokens"], 34);
        assert_eq!(p["latency_ms"], 880);
        assert!((p["cost_usd"].as_f64().unwrap() - 0.002).abs() < 1e-9);

        let cite = ChatEvent::Citation {
            id: "c1".into(),
            title: "Doc".into(),
            url: "https://x".into(),
            snippet: "hello".into(),
        };
        assert_eq!(cite.name(), "citation");
        assert_eq!(cite.payload("r")["url"], "https://x");

        // attachment maps mime -> "type" (matches the brief's payload).
        let att = ChatEvent::Attachment {
            id: "a".into(),
            name: "f.png".into(),
            mime: "image/png".into(),
            url: "blob:1".into(),
            size: 99,
        };
        assert_eq!(att.payload("r")["type"], "image/png");
    }
}
