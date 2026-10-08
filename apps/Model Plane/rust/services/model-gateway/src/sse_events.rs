//! Chat-parity rich SSE event taxonomy (docs/chat-parity-audit.md §2).
//!
//! ONE forward-compatible event set for `/v1/invoke/stream`. Each [`ChatEvent`]
//! maps to an SSE `event:` name + JSON `data:` payload the verevonv2 client
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

/// Who processed one answer, as the server stamped it. It rides on the `usage`
/// frame so the answer meta can say where an answer was processed, from
/// server-stamped values only: the client never infers residency from a model
/// name, and an older gateway simply sends nothing.
///
/// `provider_used` and `residency` are read off the final chunk (or unary
/// response) of the call that served the answer. inference-core's fallback chain
/// writes both from the serving provider's own declared capabilities, so the
/// ChatGPT-subscription route reads `global` even though that provider itself
/// returns both empty.
///
/// `privacy_tier` is the strongest tier the gateway can substantiate for this
/// answer, never an overstatement: the tier its stamped residency implies on
/// its own (`global` is `global`; `eu` and `norway` are `eu_resident`), raised
/// to the floor the request enforced (inference-core skips every provider below
/// it, so an answer that was served met it). Retention is not carried on the
/// chunk, so `zdr_contractual` and `sovereign` appear only when the request
/// demanded them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServedBy {
    /// Registry id of the provider deployment that served the answer.
    pub provider_used: String,
    /// Residency that deployment declares: `global`, `eu` or `norway`.
    pub residency: String,
    /// `global`, `eu_resident`, `zdr_contractual` or `sovereign`; `None` when
    /// nothing is asserted.
    pub privacy_tier: Option<&'static str>,
}

impl ServedBy {
    /// The provenance of an answer from what the call that served it reported.
    ///
    /// `None` when no provider served it (a synthetic answer that made no
    /// provider call, or an older inference-core that stamped nothing), so the
    /// frame then carries no provenance at all rather than an empty claim.
    /// `enforced_floor_wire` is the `PrivacyTier` ordinal the request sent
    /// with the call; anything outside 1..=4 expresses no floor.
    #[must_use]
    pub fn stamped(provider_used: &str, residency: &str, enforced_floor_wire: i32) -> Option<Self> {
        let provider_used = provider_used.trim();
        if provider_used.is_empty() {
            return None;
        }
        let residency = residency.trim();
        let implied = match residency.to_ascii_lowercase().as_str() {
            "global" => 1,
            "eu" | "norway" => 2,
            _ => 0,
        };
        let floor = if (1..=4).contains(&enforced_floor_wire) {
            enforced_floor_wire
        } else {
            0
        };
        let privacy_tier = match implied.max(floor) {
            1 => Some("global"),
            2 => Some("eu_resident"),
            3 => Some("zdr_contractual"),
            4 => Some("sovereign"),
            _ => None,
        };
        Some(Self {
            provider_used: provider_used.to_owned(),
            residency: residency.to_owned(),
            privacy_tier,
        })
    }

    /// Writes each non-empty field onto a `usage` payload, leaving the keys of
    /// every payload that predates them untouched.
    fn write_to(&self, usage: &mut Value) {
        if !self.provider_used.is_empty() {
            usage["provider_used"] = json!(self.provider_used);
        }
        if !self.residency.is_empty() {
            usage["residency"] = json!(self.residency);
        }
        if let Some(tier) = self.privacy_tier {
            usage["privacy_tier"] = json!(tier);
        }
    }
}

/// A rich chat-stream event beyond the plain text delta.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatEvent {
    /// Version-bound local checks, distinct from factual/source confidence.
    /// Emitted only after the matching result has been persisted and completed.
    ResultReceipt { receipt: Value },
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
        /// Machine-stable grouping key (chat-wave2 citations spec §2.2/§2.6):
        /// `kb:{provider_slug}` for a knowledge-base source, or
        /// `web_domain(url)` for a web/deep-research one. Every producer can
        /// always compute one, so this is not optional — an ungrouped
        /// citation would otherwise fall back to one-chip-per-source in the
        /// popover with no way to tell that was the fallback, not a real
        /// per-source grouping decision.
        source_group_id: String,
        /// The human, already-localized label for `source_group_id`
        /// (currently only set by KB grounding, from `source_label()`'s
        /// output) — so the frontend never has to re-derive a
        /// Norwegian/English provider name from a slug. `None` for a web/
        /// deep-research citation, which groups by hostname instead and
        /// renders the hostname itself as the label.
        source_group_label: Option<String>,
        /// The bracketed `[n]` this citation was assigned when it was
        /// numbered (KB grounding's `context_block`, or a web/deep-research
        /// producer's own kept-order numbering). The anchor pass
        /// (`citation_anchors.rs`) resolves a model-cited marker back to a
        /// citation by this stable number, never by array position — see
        /// the off-by-gap bug that field replaced. `None` only for a
        /// deep-research source that was never numbered (an unread lead,
        /// deliberately excluded from the numbered range).
        number: Option<u32>,
        /// The claim boundary this citation was anchored to, filled in by
        /// the post-stream anchor pass. `None` on the citation's first
        /// (pre-answer) emission; a second `Citation` frame with the same
        /// `id` carries it once the answer is complete. First-occurrence
        /// only: if the same source is cited at two disjoint claims, only
        /// the first gets a `claim_id` (mirrors `upsertCitation`'s
        /// first-write-wins dedup on the client).
        claim_id: Option<String>,
        /// The marker's own byte offset range (UTF-8, per `regex::Match`; NOT
        /// a Unicode codepoint or UTF-16 count — matters for a Norwegian
        /// character like æ/ø/å, which is 2 UTF-8 bytes) within the complete
        /// answer
        /// text, set by the same anchor-pass frame as `claim_id`.
        /// INFORMATIONAL ONLY — never used to slice or highlight the answer
        /// (the client's rendering still locates `[n]` itself via its own
        /// regex match over already-split markdown blocks, whose own start
        /// offset within the full answer is not tracked). Do not wire these
        /// into rendering without re-deriving a block-relative offset first.
        start: Option<u32>,
        /// See `start`.
        end: Option<u32>,
        /// Source-scope provenance (chat-wave2 scope spec §2.3): `"web"` for
        /// a `web_search`/deep-research/`fetch_url` producer, `"knowledge"`
        /// for KB/RAG grounding or an org-connected search
        /// (`knowledge_search` and siblings). Always set by every producer —
        /// never inferred client-side — so a `knowledge_only`/`web_only`
        /// turn's receipt and Sources panel can show, and the server can
        /// defensively reject, a citation whose family disagrees with the
        /// turn's active retrieval scope.
        origin: String,
    },
    /// Per-turn retrieval-source filter transparency frame (chat-wave2 scope
    /// spec §2.2/§2.3's "shown in the receipt" requirement). Emitted at most
    /// once per turn, only when the turn's effective `RetrievalScope`
    /// (`crate::normalize::RetrievalScope`) is not `Auto`. Consumed by
    /// `ChatPanels.tsx`'s assembled-context inspector and `ChatMessages.tsx`'s
    /// scope badge — the concrete UI surface the spec's "shown in the
    /// receipt" requirement asks for.
    SourceScopeApplied {
        /// `"knowledge_only"` or `"web_only"` — never `"auto"`, which never
        /// emits this frame at all.
        scope: String,
        /// Builtin tool names this turn's offered set excluded because of
        /// `scope`, for a concrete "Nett-kilder ekskludert" / "Organisasjons-
        /// kilder ekskludert" style disclosure rather than a vague claim.
        excluded_tools: Vec<String>,
    },
    /// Structured internal grounding payload rendered by the chat UI.
    Grounding {
        grounding: crate::retrieval::Grounding,
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
        /// How `cost_usd` was established. On a subscription turn the cost
        /// is 0.0 and the tokens are usage, not spend. `None` where this
        /// layer establishes no cost at all; the key is then omitted.
        cost_basis: Option<crate::pricing::CostBasis>,
        latency_ms: u64,
        confidence: Option<f64>,
        /// Which provider served the answer, under what declared residency and
        /// privacy tier, as stamped by the server (see [`ServedBy`]). `None`
        /// where this layer saw no serving provider; the three keys are then
        /// omitted, so older clients and every earlier payload stay valid.
        served_by: Option<ServedBy>,
    },
    /// Per-turn phase timing (Q14): ids, milliseconds and counts, never
    /// content. Emitted once, just before the terminal `done`, `error` or
    /// `stopped` frame, and gated exactly like `usage`.
    TurnTiming {
        report: crate::turn_timing::TurnTimingReport,
    },
    /// AI-generated thread title, produced once after the thread's FIRST
    /// exchange completes (sidebar summary, ChatGPT-style). Control event —
    /// never feature-gated: when a title was generated it must reach the
    /// client, and a client that predates it ignores the unknown event.
    Title { title: String },
    /// AI-generated follow-up question suggestions, produced after (almost)
    /// every non-ZDR exchange completes (composer chips, ChatGPT-style).
    /// Control event — never feature-gated, same reasoning as `Title`: when
    /// suggestions were generated they must reach the client, and an older
    /// client ignores the unknown event name.
    FollowUps { suggestions: Vec<String> },
    /// The model's structured plan-mode plan (chat-wave2-specs plan-gate
    /// §2.1), emitted once a plan-mode run finishes when its `plan.propose`
    /// tool call created one. Control event — never feature-gated: an older
    /// client that has never heard of `plan_proposed` ignores the unknown
    /// event name and falls back to today's rung-only `PlanApprovalControl`,
    /// exactly the degrade path the plan-gate rollout note calls for.
    /// `steps` is a JSON array of `{id, title, operation}`, matching
    /// `PlanStep`'s wire shape — a bare `Value` here, the same pattern
    /// `ResultReceipt.receipt` already uses, rather than a new struct only
    /// this one event needs.
    PlanProposed {
        plan_id: String,
        summary: String,
        steps: Value,
    },
    /// Deterministic "memory was used" indicator (harness-adoption §7.9,
    /// hermes-agent's `RecallStatus`, MIT): emitted once per turn, before the
    /// answer streams, when long-term memory was actually injected into the
    /// prompt. `count` is the number of discrete memories injected — never 0;
    /// a turn that recalled nothing emits no event at all, so the UI can
    /// render the indicator without an "0 memories" special case. Opt-in via
    /// the `memory` feature family: the injection itself is unconditional
    /// (prompt quality is not a client choice), only this VISIBILITY signal
    /// is gated, so older clients and plain chat are byte-identical.
    /// What the low-confidence verification pass did and what it found.
    ///
    /// The score moving without explanation is worse than no score: a reader
    /// cannot tell "we checked and found nothing" from "we never looked",
    /// and those justify very different trust. Emitted only for turns that
    /// actually ran the pass.
    Verification {
        /// `supports` | `contradicts` | `unrelated` — see
        /// `verification::SourceVerdict`.
        verdict: &'static str,
        /// Knowledge-base sources the pass confirmed against.
        kb_citations: u32,
        /// Web sources it confirmed against, when the turn allowed the web.
        web_citations: u32,
        /// Whether the pass was permitted to escalate past the knowledge base.
        web_allowed: bool,
    },
    MemoryRecall {
        count: u32,
        latency_ms: u64,
        /// What was recalled, so a reader can see and correct it — not just how
        /// much. Bounded and previewed by `memory_provenance`.
        memories: Vec<crate::memory_provenance::RecalledMemoryView>,
    },
    /// A message the user sent MID-RUN has been delivered to the agent at a
    /// tool-round boundary (`queued_input`). Emitted at delivery, not at
    /// enqueue: the POST already told the client its message was accepted, and
    /// what the client cannot otherwise know is when the agent actually saw it.
    ///
    /// A control event, deliberately ungated: the user typed this and is
    /// watching for it. Hiding it behind a feature family would reproduce the
    /// exact bug it exists to fix — a message that seems to have vanished.
    QueuedInput { messages: Vec<String> },
    /// Terminal control: generation stopped/cancelled by the user.
    Stopped { reason: String },
    /// Terminal control: a structured error (chat-parity §20). `code` is a
    /// stable machine token; `retryable` tells the client whether to offer retry.
    Error {
        code: String,
        message: String,
        retryable: bool,
    },
    /// Terminal control: a ChatGPT-subscription refusal. The same `error`
    /// frame as [`ChatEvent::Error`], plus the content-free timing the SPA
    /// needs to say when a retry can work: `retry_after_ms`, `resets_at`
    /// (RFC 3339 UTC), and `retry_at`, each omitted when unknown.
    ///
    /// `retry_at` is the absolute instant a retry could succeed (the reset,
    /// or the wait counted from when this frame was built). The frame is
    /// buffered and replayed on resume, and `retry_after_ms` is relative, so
    /// a client should prefer `retry_at` over adding the wait to its own clock.
    SubscriptionError {
        code: String,
        message: String,
        retryable: bool,
        retry_after_ms: Option<u64>,
        resets_at: Option<String>,
        retry_at: Option<String>,
    },
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
            ChatEvent::Citation { .. }
            | ChatEvent::Grounding { .. }
            | ChatEvent::Verification { .. }
            | ChatEvent::SourceScopeApplied { .. } => Some("citations"),
            ChatEvent::Artifact { .. } | ChatEvent::Attachment { .. } => Some("artifacts"),
            ChatEvent::Usage { .. }
            | ChatEvent::ResultReceipt { .. }
            | ChatEvent::TurnTiming { .. } => Some("usage"),
            ChatEvent::MemoryRecall { .. } => Some("memory"),
            // control events — always allowed (Title/FollowUps only exist when
            // the gateway actually generated them; gating on a feature family
            // would silently drop them for the plain `chat` profile, which is
            // exactly the surface the sidebar title / composer chips are for)
            ChatEvent::Title { .. }
            | ChatEvent::FollowUps { .. }
            // The structured plan-mode plan (plan-gate §2.1) — essential for
            // `PlanApprovalControl` to render the edit surface, not an
            // opt-in enhancement.
            | ChatEvent::PlanProposed { .. }
            // The user typed this and is watching for it — gating it would
            // reproduce the vanishing-message bug it exists to fix.
            | ChatEvent::QueuedInput { .. }
            | ChatEvent::Stopped { .. }
            | ChatEvent::Error { .. }
            | ChatEvent::SubscriptionError { .. } => None,
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
            ChatEvent::ResultReceipt { .. } => "result_receipt",
            ChatEvent::ReasoningDelta { .. } => "reasoning_delta",
            ChatEvent::StepUpdate { .. } => "step_update",
            ChatEvent::ToolCall { .. } => "tool_call",
            ChatEvent::ToolResult { .. } => "tool_result",
            ChatEvent::Citation { .. } => "citation",
            ChatEvent::SourceScopeApplied { .. } => "source_scope_applied",
            ChatEvent::Grounding { .. } => "grounding",
            ChatEvent::Artifact { .. } => "artifact",
            ChatEvent::Attachment { .. } => "attachment",
            ChatEvent::Usage { .. } => "usage",
            ChatEvent::TurnTiming { .. } => "turn_timing",
            ChatEvent::Verification { .. } => "verification",
            ChatEvent::MemoryRecall { .. } => "memory_recall",
            ChatEvent::Title { .. } => "title",
            ChatEvent::FollowUps { .. } => "follow_ups",
            ChatEvent::PlanProposed { .. } => "plan_proposed",
            ChatEvent::QueuedInput { .. } => "queued_input",
            ChatEvent::Stopped { .. } => "stopped",
            ChatEvent::Error { .. } | ChatEvent::SubscriptionError { .. } => "error",
        }
    }

    /// The SSE `data:` JSON payload.
    #[must_use]
    pub fn payload(&self, request_id: &str) -> Value {
        match self {
            ChatEvent::ResultReceipt { receipt } => json!({ "receipt": receipt, "request_id": request_id }),
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
                source_group_id,
                source_group_label,
                number,
                claim_id,
                start,
                end,
                origin,
            } => {
                let mut payload = json!({
                    "id": id,
                    "title": title,
                    "url": url,
                    "snippet": snippet,
                    "source_group_id": source_group_id,
                    "origin": origin,
                });
                // Kept out of the object entirely (not even `null`) when
                // absent, so the common "no anchor yet" frame stays small —
                // see the field docs on `ChatEvent::Citation`.
                if let Some(label) = source_group_label {
                    payload["source_group_label"] = json!(label);
                }
                if let Some(n) = number {
                    payload["number"] = json!(n);
                }
                if let Some(id) = claim_id {
                    payload["claim_id"] = json!(id);
                }
                if let Some(value) = start {
                    payload["start"] = json!(value);
                }
                if let Some(value) = end {
                    payload["end"] = json!(value);
                }
                payload
            }
            ChatEvent::SourceScopeApplied { scope, excluded_tools } => {
                json!({ "scope": scope, "excluded_tools": excluded_tools })
            }
            ChatEvent::Grounding { grounding } => {
                serde_json::to_value(grounding).unwrap_or_else(|_| json!({}))
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
                cost_basis,
                latency_ms,
                confidence,
                served_by,
            } => {
                let mut usage = json!({
                    "input_tokens": input_tokens,
                    "output_tokens": output_tokens,
                    "cost_usd": cost_usd,
                    "latency_ms": latency_ms,
                    "confidence": confidence,
                });
                // Optional: a client that predates it sees the same payload.
                if let Some(basis) = cost_basis {
                    usage["cost_basis"] = json!(basis.as_str());
                }
                // Optional, and per field: an empty value is left out.
                if let Some(served_by) = served_by {
                    served_by.write_to(&mut usage);
                }
                usage
            }
            // The report carries its own request id.
            ChatEvent::TurnTiming { report } => {
                serde_json::to_value(report).unwrap_or_else(|_| json!({}))
            }
            ChatEvent::Verification {
                verdict,
                kb_citations,
                web_citations,
                web_allowed,
            } => json!({
                "verdict": verdict,
                "kb_citations": kb_citations,
                "web_citations": web_citations,
                "web_allowed": web_allowed,
                "request_id": request_id,
            }),
            ChatEvent::MemoryRecall {
                count,
                latency_ms,
                memories,
            } => json!({
                "count": count,
                "latency_ms": latency_ms,
                // `role`/`origin` go out as their own strings rather than as
                // booleans: `origin` has THREE states, and the third
                // ("unrecorded") must never collapse into "stated". See
                // `memory_provenance`.
                "memories": memories
                    .iter()
                    .map(|memory| json!({
                        "memory_id": memory.memory_id,
                        "role": memory.role.as_str(),
                        "origin": memory.origin.as_str(),
                        "label": memory.label,
                        "preview": memory.preview,
                    }))
                    .collect::<Vec<_>>(),
                "request_id": request_id,
            }),
            ChatEvent::Title { title } => json!({ "title": title, "request_id": request_id }),
            ChatEvent::FollowUps { suggestions } => {
                json!({ "suggestions": suggestions, "request_id": request_id })
            }
            ChatEvent::PlanProposed {
                plan_id,
                summary,
                steps,
            } => json!({
                "plan_id": plan_id,
                "summary": summary,
                "steps": steps,
                "request_id": request_id,
            }),
            ChatEvent::QueuedInput { messages } => {
                json!({ "messages": messages, "request_id": request_id })
            }
            ChatEvent::Stopped { reason } => json!({ "reason": reason, "request_id": request_id }),
            ChatEvent::Error {
                code,
                message,
                retryable,
            } => json!({
                "code": code,
                "message": message,
                "retryable": retryable,
                "request_id": request_id,
            }),
            ChatEvent::SubscriptionError {
                code,
                message,
                retryable,
                retry_after_ms,
                resets_at,
                retry_at,
            } => {
                let mut error = json!({
                    "code": code,
                    "message": message,
                    "retryable": retryable,
                    "request_id": request_id,
                });
                // Omitted, not null, when unknown: a client that predates
                // them sees exactly the `Error` payload.
                if let Some(resets_at) = resets_at {
                    error["resets_at"] = json!(resets_at);
                }
                if let Some(retry_after_ms) = retry_after_ms {
                    error["retry_after_ms"] = json!(retry_after_ms);
                }
                if let Some(retry_at) = retry_at {
                    error["retry_at"] = json!(retry_at);
                }
                error
            }
        }
    }

    /// Encode as an axum SSE [`Event`] (name + JSON data).
    pub fn to_sse(&self, request_id: &str) -> Event {
        Event::default()
            .event(self.name())
            .data(self.payload(request_id).to_string())
    }

    /// The `error` frame for a subscription failure, worded by the caller.
    #[must_use]
    pub fn subscription_error(
        failure: &crate::subscription_failure::SubscriptionFailure,
        message: String,
    ) -> Self {
        Self::subscription_error_at(failure, message, chrono::Utc::now())
    }

    /// [`Self::subscription_error`] built at `now`, which dates `retry_at`.
    #[must_use]
    pub fn subscription_error_at(
        failure: &crate::subscription_failure::SubscriptionFailure,
        message: String,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        ChatEvent::SubscriptionError {
            code: failure.code.to_owned(),
            message,
            retryable: crate::subscription_failure::retryable(failure.code),
            retry_after_ms: failure.retry_after_ms,
            resets_at: failure.resets_at_label(),
            retry_at: failure.retry_at_label(now),
        }
    }
}

/// Emits [`ChatEvent`]s onto an SSE channel, honoring the client's opt-in
/// `features[]`. Cheap to clone the inputs; holds the sender + gating context.
pub struct RichEventSink {
    stage_text_artifacts: bool,
    tx: Sender<Result<Event, Infallible>>,
    features: Vec<String>,
    request_id: String,
    /// Every citation that passed through this sink, kept so the turn's
    /// evidence can be persisted with the assistant message.
    ///
    /// The tool loop streams citations straight out through `emit` and returns
    /// an EMPTY buffer whenever a live sink is supplied (see
    /// `DeepResearchOutcome.events`), so this is the only place a web-search
    /// turn's citations can still be observed. Without it, only Data Plane
    /// `Grounding` was persistable and every tool-sourced source vanished from
    /// history the moment the browser cache was gone.
    citations: std::sync::Arc<std::sync::Mutex<Vec<RecordedCitation>>>,
    /// Every text artifact that passed through this sink, for the same reason
    /// as `citations`: the durable thread read returned zero artifacts for a
    /// turn that had produced three (RUN-LOG finding 3), because the only
    /// copy lived in the browser that watched the stream. Generated binaries
    /// (data: URIs) are NOT recorded — see [`RecordedArtifact::from_event`].
    artifacts: std::sync::Arc<std::sync::Mutex<Vec<RecordedArtifact>>>,
    /// The turn's clock, shared the same way, so the tool loop and the draft
    /// checks it runs can time their calls without new parameters.
    timing: Option<crate::turn_timing::TurnTiming>,
    /// The stream's resume buffer and its shared frame cursor, when the caller
    /// attached them. Tool calls, tool results and steps used to go out on the
    /// live channel only, so a reload during the tool phase, source review or
    /// repair resumed into a stream that had none of that progress.
    resume: Option<SinkResume>,
    /// A typed subscription refusal the tool phase ended on, and what it was
    /// producing. The tool loop can only return a `&'static str` reason, so the
    /// timing that came with the refusal waits here for the stream handler.
    subscription_failure: std::sync::Mutex<
        Option<(
            crate::subscription_failure::SubscriptionFailure,
            crate::subscription_failure::FailureContext,
        )>,
    >,
}

/// Where a sink buffers what it sends: the same store, key and `id:` cursor
/// the stream's own frames use, so one resume cursor orders them all.
struct SinkResume {
    buffers: crate::stream_buffer::StreamBufferStore,
    key: String,
    /// The next frame id. Held across append and send so concurrent emits
    /// (a round's parallel tool results) reach the buffer and the wire in the
    /// same order.
    next_seq: tokio::sync::Mutex<u64>,
}

/// A generated image or file, whose payload is an inline `data:` URI.
fn carries_binary(event: &ChatEvent) -> bool {
    match event {
        ChatEvent::Artifact { content, .. } => content.starts_with("data:"),
        ChatEvent::Attachment { url, .. } => url.starts_with("data:"),
        _ => false,
    }
}

/// A citation observed on the wire, in the shape the SPA already reads back.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RecordedCitation {
    pub id: String,
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub source_group_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_group_label: Option<String>,
    /// The `[n]` this citation was assigned when numbered — see
    /// `ChatEvent::Citation::number`. Used by `citation_anchors::anchor_citations`
    /// to resolve a model-cited marker back to this citation by stable
    /// number rather than by position in this vec.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<u32>,
    /// See `ChatEvent::Citation::origin`.
    pub origin: String,
}

/// An authored artifact observed on the wire, in the shape the SPA's
/// `isChatArtifact` accepts back from a thread read.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RecordedArtifact {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub content: String,
    pub version: u32,
}

impl RecordedArtifact {
    /// Largest artifact body persisted with a message. Matches the authoring
    /// cap in `artifacts::MAX_TEXT_ARTIFACT_CHARS`, so nothing the model may
    /// write is ever too big to keep.
    pub const MAX_PERSISTED_CHARS: usize = 200_000;

    /// `Some` for a model-authored text artifact (document/code/html);
    /// `None` for generated files and images, whose `content` is a base64
    /// `data:` URI far too large for a metadata column and already
    /// downloadable from the message's attachment.
    #[must_use]
    pub fn from_event(event: &ChatEvent) -> Option<Self> {
        let ChatEvent::Artifact {
            id,
            kind,
            title,
            content,
            version,
        } = event
        else {
            return None;
        };
        if !matches!(kind.as_str(), "document" | "code" | "html") {
            return None;
        }
        if content.starts_with("data:") || content.chars().count() > Self::MAX_PERSISTED_CHARS {
            return None;
        }
        Some(Self {
            id: id.clone(),
            kind: kind.clone(),
            title: title.clone(),
            content: content.clone(),
            version: *version,
        })
    }
}

impl RichEventSink {
    #[must_use]
    pub fn new(
        tx: Sender<Result<Event, Infallible>>,
        features: Vec<String>,
        request_id: String,
    ) -> Self {
        Self {
            stage_text_artifacts: false,
            tx,
            features,
            request_id,
            citations: std::sync::Arc::default(),
            artifacts: std::sync::Arc::default(),
            timing: None,
            resume: None,
            subscription_failure: std::sync::Mutex::new(None),
        }
    }

    /// Keep the typed refusal a tool phase ended on. The first one is kept:
    /// it is what stopped the phase.
    pub fn set_subscription_failure(
        &self,
        failure: crate::subscription_failure::SubscriptionFailure,
        context: crate::subscription_failure::FailureContext,
    ) {
        if let Ok(mut slot) = self.subscription_failure.lock() {
            slot.get_or_insert((failure, context));
        }
    }

    /// Take the typed refusal the tool phase ended on, if it ended on one.
    pub fn take_subscription_failure(
        &self,
    ) -> Option<(
        crate::subscription_failure::SubscriptionFailure,
        crate::subscription_failure::FailureContext,
    )> {
        self.subscription_failure
            .lock()
            .ok()
            .and_then(|mut slot| slot.take())
    }

    /// Text artifacts are private candidates until the caller persists the
    /// completed assistant message and terminalizes the run. Progress/citations
    /// continue to stream. Dropping this sink discards unpublished candidates.
    pub fn staging_artifacts(
        tx: Sender<Result<Event, Infallible>>, features: Vec<String>, request_id: String,
    ) -> Self {
        Self { stage_text_artifacts: true, ..Self::new(tx, features, request_id) }
    }

    /// Attach the turn's clock.
    #[must_use]
    pub fn with_timing(mut self, timing: crate::turn_timing::TurnTiming) -> Self {
        self.timing = Some(timing);
        self
    }

    /// The turn's clock, when the caller attached one.
    #[must_use]
    pub fn timing(&self) -> Option<&crate::turn_timing::TurnTiming> {
        self.timing.as_ref()
    }

    /// Buffer every frame this sink sends for resume, under the stream's own
    /// `id:` sequence starting at `next_seq`. The caller hands its cursor over
    /// and takes it back with [`sync_cursor`](Self::sync_cursor) around every
    /// stretch in which the sink emits.
    #[must_use]
    pub fn buffering(
        mut self,
        buffers: crate::stream_buffer::StreamBufferStore,
        key: String,
        next_seq: u64,
    ) -> Self {
        self.resume = Some(SinkResume {
            buffers,
            key,
            next_seq: tokio::sync::Mutex::new(next_seq),
        });
        self
    }

    /// Reconcile the caller's frame cursor with this sink's and return the
    /// shared value. Both only ever grow and never emit at the same time, so
    /// the larger one is always the next free id: calling this before handing
    /// the sink to the tool loop gives the sink the caller's cursor, and
    /// calling it after gives the caller the sink's. Without a buffer the
    /// cursor comes back unchanged.
    pub async fn sync_cursor(&self, seq: u64) -> u64 {
        let Some(resume) = &self.resume else {
            return seq;
        };
        let mut next = resume.next_seq.lock().await;
        *next = (*next).max(seq);
        *next
    }

    /// Record citations that reached the wire BEFORE this sink existed,
    /// without re-emitting them.
    ///
    /// chat-parity §8 emits KB/RAG grounding's citations "up front" (so the
    /// Sources panel can render before the answer streams), and it does so
    /// via the free-standing `emit_and_buffer` helper directly on the
    /// stream's `tx`/buffer — at that point in the handler `RichEventSink`
    /// has not been constructed yet. `emit()` is the ONLY place that
    /// normally appends to `self.citations`, so without this call the
    /// up-front citations would never appear in `recorded_citations()` —
    /// silently starving both the post-stream anchor pass
    /// (`citation_anchors::anchor_citations`, chat-wave2 citations spec
    /// §2.4) and persisted `turn_evidence_metadata`'s `citations` field of
    /// the single most common citation producer. Call this once, right
    /// after constructing the sink, with the same citations already sent.
    pub fn seed_recorded_citations(&self, citations: impl IntoIterator<Item = RecordedCitation>) {
        if let Ok(mut recorded) = self.citations.lock() {
            recorded.extend(citations);
        }
    }

    /// Emit an event if the client opted into its family (control events always
    /// emit). Best-effort: a closed channel (client gone) is ignored.
    ///
    /// With a resume buffer attached the frame is buffered first and sent
    /// with the next `id:`. Staged documents never get here, and generated
    /// binaries (a `data:` image or file) stay live-only as on the image
    /// route: one such frame can be megabytes, and the saved message and its
    /// attachment are how a reload gets it back.
    pub async fn emit(&self, event: ChatEvent) {
        // Recorded BEFORE the feature gate on purpose: which sources a turn
        // used is a fact about the turn, not about what this client opted into
        // receiving. Persisting only what one client subscribed to would make
        // durable history depend on the shape of the request that created it.
        if let ChatEvent::Citation {
            id,
            title,
            url,
            snippet,
            source_group_id,
            source_group_label,
            number,
            origin,
            ..
        } = &event
        {
            if let Ok(mut recorded) = self.citations.lock() {
                recorded.push(RecordedCitation {
                    id: id.clone(),
                    title: title.clone(),
                    url: url.clone(),
                    snippet: snippet.clone(),
                    source_group_id: source_group_id.clone(),
                    source_group_label: source_group_label.clone(),
                    number: *number,
                    origin: origin.clone(),
                });
            }
        }
        if let Some(artifact) = RecordedArtifact::from_event(&event) {
            if let Ok(mut recorded) = self.artifacts.lock() {
                recorded.push(artifact);
            }
            if self.stage_text_artifacts { return; }
        }
        if !event.should_emit(&self.features) {
            return;
        }
        if let Some(resume) = self.resume.as_ref().filter(|_| !carries_binary(&event)) {
            let name = event.name();
            let data = event.payload(&self.request_id).to_string();
            let mut next = resume.next_seq.lock().await;
            // Buffer before sending, as `emit_and_buffer` does, so a reconnect
            // can never observe a frame the buffer does not have.
            resume.buffers.append(&resume.key, *next, name, &data).await;
            let _ = self
                .tx
                .send(Ok(Event::default()
                    .id(next.to_string())
                    .event(name)
                    .data(data)))
                .await;
            *next += 1;
            return;
        }
        let _ = self.tx.send(Ok(event.to_sse(&self.request_id))).await;
    }

    /// The artifacts seen so far, one entry per id carrying its LATEST version,
    /// in first-seen order. A reopened thread needs the final document, not
    /// every draft the loop went through.
    #[must_use]
    pub fn recorded_artifacts(&self) -> Vec<RecordedArtifact> {
        let Ok(recorded) = self.artifacts.lock() else {
            return Vec::new();
        };
        let mut order: Vec<String> = Vec::new();
        let mut latest: std::collections::HashMap<String, RecordedArtifact> =
            std::collections::HashMap::new();
        for artifact in recorded.iter() {
            match latest.get(&artifact.id) {
                Some(existing) if existing.version >= artifact.version => {}
                Some(_) => {
                    latest.insert(artifact.id.clone(), artifact.clone());
                }
                None => {
                    order.push(artifact.id.clone());
                    latest.insert(artifact.id.clone(), artifact.clone());
                }
            }
        }
        order
            .into_iter()
            .filter_map(|id| latest.remove(&id))
            .collect()
    }

    /// The citations seen so far, deduplicated by url (falling back to id) in
    /// first-seen order — the tool loop can legitimately re-cite one source
    /// across rounds, and history should list it once.
    #[must_use]
    pub fn recorded_citations(&self) -> Vec<RecordedCitation> {
        let Ok(recorded) = self.citations.lock() else {
            return Vec::new();
        };
        let mut seen = std::collections::HashSet::new();
        recorded
            .iter()
            .filter(|citation| {
                let key = if citation.url.is_empty() {
                    &citation.id
                } else {
                    &citation.url
                };
                seen.insert(key.clone())
            })
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn staged_artifacts_never_reach_the_wire_on_a_failed_turn() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let sink = RichEventSink::staging_artifacts(tx, families(&["artifacts", "steps"]), "r".into());
        for version in [3, 4] {
            sink.emit(ChatEvent::Artifact { id: "doc".into(), kind: "document".into(),
                title: "Draft".into(), content: format!("private {version}"), version }).await;
        }
        assert!(rx.try_recv().is_err());
        assert_eq!(sink.recorded_artifacts()[0].version, 4);
        sink.emit(ChatEvent::Error { code: "response_validation_failed".into(),
            message: "Not completed".into(), retryable: true }).await;
        assert!(rx.try_recv().is_ok(), "failure still reaches the browser");
        drop(sink);
        assert!(rx.try_recv().is_err(), "dropping a failed run never releases drafts");
    }

    fn families(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    /// A subscription refusal is an ordinary, ungated `error` frame with two
    /// optional fields, each left out rather than sent as null.
    #[test]
    fn a_subscription_error_is_an_error_frame_that_omits_unknown_timing() {
        use crate::subscription_failure::{SubscriptionFailure, SUBSCRIPTION_USAGE_LIMIT};
        let failure = SubscriptionFailure {
            code: SUBSCRIPTION_USAGE_LIMIT,
            retry_after_ms: Some(52_260_000),
            resets_at: chrono::DateTime::parse_from_rfc3339("2026-09-26T01:21:00Z")
                .ok()
                .map(|at| at.with_timezone(&chrono::Utc)),
        };
        let event = ChatEvent::subscription_error(&failure, "Grensen er nådd.".into());
        assert_eq!(event.name(), "error");
        assert_eq!(event.family(), None);
        assert!(event.should_emit(&[]), "the plain chat profile receives it");
        assert_eq!(
            event.payload("req-1"),
            serde_json::json!({
                "code": "subscription_usage_limit",
                "message": "Grensen er nådd.",
                "retryable": false,
                "resets_at": "2026-09-26T01:21:00Z",
                "retry_after_ms": 52_260_000_u64,
                "retry_at": "2026-09-26T01:21:00Z",
                "request_id": "req-1",
            })
        );

        // A wait alone is dated when the frame is built, so a replay of the
        // buffered frame names the same instant rather than a later one.
        let built = chrono::DateTime::parse_from_rfc3339("2026-09-25T10:50:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let throttled = ChatEvent::subscription_error_at(
            &SubscriptionFailure {
                retry_after_ms: Some(1_500),
                ..SubscriptionFailure::bare(crate::subscription_failure::RATE_LIMITED)
            },
            "Vent litt.".into(),
            built,
        );
        let payload = throttled.payload("req-1");
        assert_eq!(payload["retry_after_ms"], 1_500);
        assert_eq!(payload["retry_at"], "2026-09-25T10:50:02Z");
        assert!(payload.get("resets_at").is_none(), "{payload}");

        let bare = ChatEvent::subscription_error(
            &SubscriptionFailure::bare(crate::subscription_failure::SUBSCRIPTION_UPSTREAM_UNAVAILABLE),
            "Prøv igjen.".into(),
        );
        let plain = ChatEvent::Error {
            code: "subscription_upstream_unavailable".into(),
            message: "Prøv igjen.".into(),
            retryable: true,
        };
        assert_eq!(
            bare.payload("req-1"),
            plain.payload("req-1"),
            "with no timing it is exactly the Error payload"
        );
    }

    #[test]
    fn the_sink_keeps_the_first_subscription_failure_until_it_is_taken() {
        use crate::subscription_failure::{FailureContext, SubscriptionFailure};
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let sink = RichEventSink::new(tx, Vec::new(), "r".into());
        assert!(sink.take_subscription_failure().is_none());
        sink.set_subscription_failure(
            SubscriptionFailure::bare("subscription_reauth_required"),
            FailureContext::Draft,
        );
        sink.set_subscription_failure(
            SubscriptionFailure::bare("rate_limited"),
            FailureContext::ToolRound,
        );
        assert_eq!(
            sink.take_subscription_failure(),
            Some((
                SubscriptionFailure::bare("subscription_reauth_required"),
                FailureContext::Draft
            ))
        );
        assert!(sink.take_subscription_failure().is_none());
    }

    /// Q15: tool calls, tool results and steps went out on the live channel
    /// only, so a reload during the tool phase or source review resumed with
    /// none of that progress. A buffering sink continues the stream's own
    /// cursor; staged drafts, gated events and generated binaries take no id.
    #[tokio::test]
    async fn a_buffering_sink_continues_the_stream_cursor() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let buffers = crate::stream_buffer::StreamBufferStore::new();
        let features = families(&["tools", "steps", "artifacts"]);
        let sink = RichEventSink::staging_artifacts(tx, features, "r".into()).buffering(
            buffers.clone(),
            "key".into(),
            0,
        );
        // The stream buffered frames 0..3 itself before the tool phase.
        assert_eq!(sink.sync_cursor(3).await, 3);
        sink.emit(ChatEvent::StepUpdate {
            id: "s".into(),
            title: "Kontrollerer".into(),
            detail: String::new(),
            status: "running".into(),
        })
        .await;
        sink.emit(ChatEvent::ToolCall {
            id: "c".into(),
            name: "web_search".into(),
            args: json!({}),
        })
        .await;
        let artifact = |id: &str, kind: &str, content: &str| ChatEvent::Artifact {
            id: id.into(),
            kind: kind.into(),
            title: "T".into(),
            content: content.into(),
            version: 1,
        };
        sink.emit(artifact("doc", "document", "private draft"))
            .await;
        sink.emit(ChatEvent::ReasoningDelta {
            delta: "not opted in".into(),
        })
        .await;
        sink.emit(artifact("img", "image", "data:image/png;base64,AAAA"))
            .await;
        assert_eq!(
            sink.sync_cursor(0).await,
            5,
            "the stream continues after the sink's frames"
        );

        let replay = buffers.replay_after("key", None).await;
        let buffered: Vec<(u64, &str)> = replay
            .deltas
            .iter()
            .map(|frame| (frame.seq, frame.event.as_str()))
            .collect();
        assert_eq!(buffered, vec![(3, "step_update"), (4, "tool_call")]);
        // Live: the step, the tool call and the image; never the staged draft.
        let mut live = 0;
        while rx.try_recv().is_ok() {
            live += 1;
        }
        assert_eq!(live, 3);
    }

    /// The anchor pass (`citation_anchors.rs`) re-emits a `Citation` frame
    /// with the SAME `id` once the answer is complete, now carrying
    /// `claim_id`. `SinkResume`'s buffer is generic over any `ChatEvent` and
    /// keys purely on the stream's own sequence number, never on a citation's
    /// logical id, so a reconnect must replay BOTH frames in order — a
    /// client that only saw the buffered replay must still end up with the
    /// anchored citation, the same as one that watched the live stream (see
    /// `chat-normalizers.ts::upsertCitation`'s merge-on-duplicate-id change).
    #[tokio::test]
    async fn a_repeated_citation_id_is_buffered_and_replayed_both_times() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let buffers = crate::stream_buffer::StreamBufferStore::new();
        let sink = RichEventSink::new(tx, families(&["citations"]), "r".into()).buffering(
            buffers.clone(),
            "key".into(),
            0,
        );
        let first = ChatEvent::Citation {
            id: "c1".into(),
            title: "Doc".into(),
            url: "https://kb/doc".into(),
            snippet: "hello".into(),
            source_group_id: "kb:sharepoint".into(),
            source_group_label: Some("Microsoft 365".into()),
            number: Some(1),
            claim_id: None,
            start: None,
            end: None,
            origin: "knowledge".into(),
        };
        let anchored = ChatEvent::Citation {
            id: "c1".into(),
            title: "Doc".into(),
            url: "https://kb/doc".into(),
            snippet: "hello".into(),
            source_group_id: "kb:sharepoint".into(),
            source_group_label: Some("Microsoft 365".into()),
            number: Some(1),
            claim_id: Some("r:claim-0".into()),
            start: Some(4),
            end: Some(7),
            origin: "knowledge".into(),
        };
        sink.emit(first).await;
        sink.emit(anchored).await;

        let replay = buffers.replay_after("key", None).await;
        let buffered: Vec<(u64, &str)> = replay
            .deltas
            .iter()
            .map(|frame| (frame.seq, frame.event.as_str()))
            .collect();
        assert_eq!(
            buffered,
            vec![(0, "citation"), (1, "citation")],
            "both the original and the anchor-pass update must be buffered"
        );
        assert!(!replay.deltas[0].data.contains("claim_id"));
        assert!(replay.deltas[1].data.contains("r:claim-0"));

        let mut live = Vec::new();
        while let Ok(Ok(event)) = rx.try_recv() {
            live.push(event);
        }
        assert_eq!(live.len(), 2, "both frames also reach the live channel");
    }

    /// Regression: KB/RAG grounding's citations reach the wire via the
    /// free-standing `emit_and_buffer` helper before `RichEventSink` exists
    /// (chat-parity §8's up-front emission, `sse.rs` around its `sink`
    /// construction). Without `seed_recorded_citations`, those citations
    /// never entered `self.citations`, so `recorded_citations()` — and the
    /// post-stream anchor pass built on it (chat-wave2 citations spec §2.4)
    /// — silently never saw the single most common citation producer.
    #[tokio::test]
    async fn seeded_citations_appear_in_recorded_citations_without_reaching_the_wire() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let sink = RichEventSink::new(tx, families(&["citations"]), "r".into());
        sink.seed_recorded_citations([RecordedCitation {
            id: "kb-1".into(),
            title: "Policy".into(),
            url: "https://kb/policy".into(),
            snippet: "Refunds within 30 days.".into(),
            source_group_id: "kb:sharepoint".into(),
            source_group_label: Some("Microsoft 365".into()),
            number: Some(1),
            origin: "knowledge".into(),
        }]);
        let recorded = sink.recorded_citations();
        assert_eq!(recorded.len(), 1, "the seeded citation must be visible to recorded_citations()");
        assert_eq!(recorded[0].id, "kb-1");
        assert_eq!(recorded[0].source_group_id, "kb:sharepoint");
        // Seeding records without emitting: it describes what already went
        // out via `emit_and_buffer` directly, so a live-channel send here
        // would duplicate the frame the client already received.
        assert!(rx.try_recv().is_err(), "seeding must never itself send a frame");
    }

    #[tokio::test]
    async fn a_sink_without_a_buffer_leaves_the_cursor_alone() {
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        let sink = RichEventSink::new(tx, families(&["tools"]), "r".into());
        sink.emit(ChatEvent::ToolCall {
            id: "c".into(),
            name: "web_search".into(),
            args: json!({}),
        })
        .await;
        assert_eq!(sink.sync_cursor(7).await, 7);
    }

    /// The durable read returned zero artifacts for a turn that produced
    /// three (RUN-LOG finding 3). The sink must keep what went out — latest
    /// version per id, drafts collapsed, binaries excluded.
    #[tokio::test]
    async fn sink_records_the_latest_version_of_each_text_artifact() {
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let sink = RichEventSink::new(tx, families(&["artifacts"]), "req".into());
        let artifact = |id: &str, kind: &str, content: &str, version: u32| ChatEvent::Artifact {
            id: id.into(),
            kind: kind.into(),
            title: format!("T {id}"),
            content: content.into(),
            version,
        };
        sink.emit(artifact("rapport", "document", "v1 body", 1)).await;
        sink.emit(artifact("notat", "document", "memo", 1)).await;
        sink.emit(artifact("rapport", "document", "v2 body", 2)).await;
        // A generated file is a data: URI and never belongs in metadata.
        sink.emit(artifact("chart", "image", "data:image/png;base64,AAAA", 1)).await;
        sink.emit(artifact("sheet", "spreadsheet", "data:application/x;base64,AA", 1)).await;

        let recorded = sink.recorded_artifacts();
        assert_eq!(recorded.len(), 2, "one entry per text artifact id");
        assert_eq!(recorded[0].id, "rapport", "first-seen order is kept");
        assert_eq!(recorded[0].version, 2);
        assert_eq!(recorded[0].content, "v2 body", "the latest draft wins");
        assert_eq!(recorded[1].id, "notat");
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
            cost_basis: None,
            latency_ms: 10,
            confidence: None,
            served_by: None,
        }
        .should_emit(&no_features));
        assert!(ChatEvent::Stopped {
            reason: "user".into()
        }
        .should_emit(&no_features));
        // Title is a control event: a generated title must reach the plain
        // `chat` profile — the sidebar is exactly that surface.
        let title = ChatEvent::Title {
            title: "Visma fakturastatus".into(),
        };
        assert!(title.should_emit(&no_features));
        assert_eq!(title.name(), "title");
        assert_eq!(title.payload("req-9")["title"], "Visma fakturastatus");
        assert_eq!(title.payload("req-9")["request_id"], "req-9");
        // FollowUps is a control event too — same reasoning as Title: the
        // composer chips are a plain-chat surface, not an opt-in feature.
        let follow_ups = ChatEvent::FollowUps {
            suggestions: vec!["Hva med frakt til Bergen?".into()],
        };
        assert!(follow_ups.should_emit(&no_features));
        assert_eq!(follow_ups.name(), "follow_ups");
        assert_eq!(
            follow_ups.payload("req-9")["suggestions"][0],
            "Hva med frakt til Bergen?"
        );
        assert_eq!(follow_ups.payload("req-9")["request_id"], "req-9");
        // plan_proposed is a control event too (plan-gate §2.1): essential
        // for PlanApprovalControl's edit surface, not an opt-in enhancement.
        let plan_proposed = ChatEvent::PlanProposed {
            plan_id: "plan_abc123".into(),
            summary: "Send the overdue invoice".into(),
            steps: json!([{ "id": "step_1", "title": "Draft the email", "operation": "" }]),
        };
        assert!(plan_proposed.should_emit(&no_features));
        assert_eq!(plan_proposed.name(), "plan_proposed");
        assert_eq!(
            plan_proposed.payload("req-9")["plan_id"],
            "plan_abc123"
        );
        assert_eq!(
            plan_proposed.payload("req-9")["summary"],
            "Send the overdue invoice"
        );
        assert_eq!(
            plan_proposed.payload("req-9")["steps"][0]["title"],
            "Draft the email"
        );
        assert_eq!(plan_proposed.payload("req-9")["request_id"], "req-9");
        // Error is a control event too — it must reach the plain path so a
        // profile:"chat" client still learns the stream failed (chat-parity §20).
        let err = ChatEvent::Error {
            code: "model_plane_unavailable".into(),
            message: "boom".into(),
            retryable: true,
        };
        assert!(err.should_emit(&no_features));
        assert_eq!(err.name(), "error");
        let payload = err.payload("req-1");
        assert_eq!(payload["code"], "model_plane_unavailable");
        assert_eq!(payload["message"], "boom");
        assert_eq!(payload["retryable"], true);
        assert_eq!(payload["request_id"], "req-1");
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
            cost_basis: Some(crate::pricing::CostBasis::Metered),
            latency_ms: 5,
            confidence: Some(0.9),
            served_by: None,
        }
        .should_emit(&feats));
        // Not opted in:
        assert!(!ChatEvent::Citation {
            id: "c".into(),
            title: "t".into(),
            url: "u".into(),
            snippet: "s".into(),
            source_group_id: "web:u".into(),
            source_group_label: None,
            number: Some(1),
            claim_id: None,
            start: None,
            end: None,
            origin: "web".into(),
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
            cost_basis: Some(crate::pricing::CostBasis::Metered),
            latency_ms: 880,
            confidence: Some(0.77),
            served_by: None,
        };
        assert_eq!(usage.name(), "usage");
        let p = usage.payload("req-1");
        assert_eq!(p["input_tokens"], 12);
        assert_eq!(p["output_tokens"], 34);
        assert_eq!(p["latency_ms"], 880);
        assert!((p["cost_usd"].as_f64().unwrap() - 0.002).abs() < 1e-9);
        assert_eq!(p["cost_basis"], "metered");

        let cite = ChatEvent::Citation {
            id: "c1".into(),
            title: "Doc".into(),
            url: "https://x".into(),
            snippet: "hello".into(),
            source_group_id: "kb:sharepoint".into(),
            source_group_label: Some("Microsoft 365".into()),
            number: Some(1),
            claim_id: None,
            start: None,
            end: None,
            origin: "knowledge".into(),
        };
        assert_eq!(cite.name(), "citation");
        let cite_payload = cite.payload("r");
        assert_eq!(cite_payload["url"], "https://x");
        assert_eq!(cite_payload["source_group_id"], "kb:sharepoint");
        assert_eq!(cite_payload["source_group_label"], "Microsoft 365");
        assert_eq!(cite_payload["number"], 1);
        assert_eq!(cite_payload["origin"], "knowledge");
        // Absent claim_id/start/end are left out of the object entirely
        // (not even `null`), so the common pre-anchor-pass frame stays small.
        assert!(cite_payload.get("claim_id").is_none());
        assert!(cite_payload.get("start").is_none());
        assert!(cite_payload.get("end").is_none());

        let anchored = ChatEvent::Citation {
            id: "c1".into(),
            title: "Doc".into(),
            url: "https://x".into(),
            snippet: "hello".into(),
            source_group_id: "kb:sharepoint".into(),
            source_group_label: Some("Microsoft 365".into()),
            number: Some(1),
            claim_id: Some("turn-1:claim-0".into()),
            start: Some(12),
            end: Some(15),
            origin: "knowledge".into(),
        };
        let anchored_payload = anchored.payload("r");
        assert_eq!(anchored_payload["claim_id"], "turn-1:claim-0");
        assert_eq!(anchored_payload["start"], 12);
        assert_eq!(anchored_payload["end"], 15);

        let grounding = ChatEvent::Grounding {
            grounding: crate::retrieval::Grounding {
                mode: "hybrid".into(),
                query: "refund policy".into(),
                trace_id: Some("trace-1".into()),
                low_confidence: false,
                fact_count: 1,
                source_count: 1,
                suggested_next_tools: Vec::new(),
                zdr_actions_applied: Vec::new(),
                facts: vec![crate::retrieval::GroundingFact {
                    knowledge_id: "kid-1".into(),
                    document_id: "doc-1".into(),
                    text: "Refunds are accepted within 30 days.".into(),
                    score: 0.93,
                    source_title: "Refund policy".into(),
                    source_type: "policy".into(),
                    provider: "Notion".into(),
                    chunk_index: 0,
                }],
                sources: vec![crate::retrieval::GroundingSource {
                    id: "doc-1".into(),
                    kind: "knowledge".into(),
                    title: "Refund policy".into(),
                    snippet: "Refunds are accepted within 30 days.".into(),
                    provider: "Notion".into(),
                    source_type: "policy".into(),
                    document_id: "doc-1".into(),
                    href: "/knowledge".into(),
                    score: 0.93,
                }],
                graph: Some(crate::retrieval::GroundingGraph {
                    trace_id: Some("graph-1".into()),
                    community_summaries: vec!["Refund policy connects with return workflow.".into()],
                    edge_count: 0,
                    nodes: vec![crate::retrieval::GroundingGraphNode {
                        id: "node-1".into(),
                        label: "Refund policy".into(),
                        kind: "policy".into(),
                    }],
                }),
                context_block: String::new(),
                citations: Vec::new(),
            },
        };
        assert_eq!(grounding.family(), Some("citations"));
        assert_eq!(grounding.name(), "grounding");
        assert_eq!(grounding.payload("r")["mode"], "hybrid");
        assert_eq!(grounding.payload("r")["graph"]["traceId"], "graph-1");

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

    #[test]
    fn usage_cost_basis_explains_a_subscription_cost_and_is_otherwise_optional() {
        // A subscription turn keeps `cost_usd` at 0.0 for older clients and
        // says why; a turn with no established basis keeps the old keys.
        let subscription = ChatEvent::Usage {
            input_tokens: 1_800,
            output_tokens: 120,
            cost_usd: Some(0.0),
            cost_basis: Some(crate::pricing::CostBasis::Subscription),
            latency_ms: 880,
            confidence: None,
            served_by: None,
        }
        .payload("req-2");
        assert_eq!(subscription["cost_usd"], 0.0);
        assert_eq!(subscription["cost_basis"], "subscription");
        assert_eq!(subscription["input_tokens"], 1_800);
        let unestablished = ChatEvent::Usage {
            input_tokens: 0,
            output_tokens: 0,
            cost_usd: None,
            cost_basis: None,
            latency_ms: 880,
            confidence: None,
            served_by: None,
        }
        .payload("req-3");
        let mut keys: Vec<&str> = unestablished
            .as_object()
            .expect("usage payload is an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "confidence",
                "cost_usd",
                "input_tokens",
                "latency_ms",
                "output_tokens"
            ]
        );
    }

    fn usage_with(served_by: Option<ServedBy>) -> serde_json::Value {
        ChatEvent::Usage {
            input_tokens: 10,
            output_tokens: 5,
            cost_usd: Some(0.0),
            cost_basis: Some(crate::pricing::CostBasis::Metered),
            latency_ms: 700,
            confidence: None,
            served_by,
        }
        .payload("req-served")
    }

    /// The usage payload carries the three provenance fields when the server
    /// stamped them, and exactly the keys it always had when it did not.
    #[test]
    fn usage_carries_server_stamped_provenance_and_omits_it_when_empty() {
        let baseline = usage_with(None);
        let mut baseline_keys: Vec<&str> = baseline
            .as_object()
            .expect("usage payload is an object")
            .keys()
            .map(String::as_str)
            .collect();
        baseline_keys.sort_unstable();
        assert_eq!(
            baseline_keys,
            [
                "confidence",
                "cost_basis",
                "cost_usd",
                "input_tokens",
                "latency_ms",
                "output_tokens"
            ],
            "no provenance, no new keys"
        );

        let azure = usage_with(ServedBy::stamped("azure-openai", "eu", 3));
        assert_eq!(azure["provider_used"], "azure-openai");
        assert_eq!(azure["residency"], "eu");
        assert_eq!(azure["privacy_tier"], "zdr_contractual");
        // The fields sit beside the old ones; none of those moved.
        assert_eq!(azure["input_tokens"], 10);
        assert_eq!(azure["cost_basis"], "metered");

        // Per field: an empty value is left out, not sent as "" or null.
        let bare = usage_with(Some(ServedBy {
            provider_used: "azure-openai".into(),
            residency: String::new(),
            privacy_tier: None,
        }));
        assert_eq!(bare["provider_used"], "azure-openai");
        assert!(bare.get("residency").is_none());
        assert!(bare.get("privacy_tier").is_none());
        let empty = usage_with(Some(ServedBy::default()));
        for key in ["provider_used", "residency", "privacy_tier"] {
            assert!(empty.get(key).is_none(), "{key} must be omitted when empty");
        }
        assert_eq!(empty, baseline, "an all-empty stamp is the old payload");
    }

    /// A subscription answer reads `global` with no tier above it. The
    /// provider itself returns both fields empty; inference-core's chain
    /// stamps them (pinned by its own test), and this is what the gateway
    /// makes of that stamp.
    #[test]
    fn a_subscription_answer_reads_global_with_no_eu_or_zdr_tier() {
        let served = ServedBy::stamped("openai-codex-subscription", "global", 0)
            .expect("a provider served the answer");
        assert_eq!(served.provider_used, "openai-codex-subscription");
        assert_eq!(served.residency, "global");
        assert_eq!(served.privacy_tier, Some("global"));
        let payload = usage_with(Some(served));
        assert_eq!(payload["residency"], "global");
        assert_eq!(payload["privacy_tier"], "global");
        let text = payload.to_string();
        assert!(
            !text.contains("eu_resident") && !text.contains("zdr") && !text.contains("sovereign"),
            "the subscription route must never carry a stronger tier: {text}"
        );
    }

    /// The tier is a lower bound: what the stamped residency implies, raised
    /// to the floor the request enforced, never more.
    #[test]
    fn served_by_asserts_only_what_residency_and_the_enforced_floor_support() {
        let tier = |provider: &str, residency: &str, floor: i32| {
            ServedBy::stamped(provider, residency, floor).and_then(|served| served.privacy_tier)
        };
        // Residency alone.
        assert_eq!(tier("azure-openai", "eu", 0), Some("eu_resident"));
        assert_eq!(tier("azure-openai", " EU ", 0), Some("eu_resident"));
        assert_eq!(tier("some-sovereign", "norway", 0), Some("eu_resident"));
        assert_eq!(tier("openai", "global", 0), Some("global"));
        // The enforced floor raises it: inference-core skipped every provider
        // below it, so the answer met it.
        assert_eq!(tier("azure-openai", "eu", 3), Some("zdr_contractual"));
        assert_eq!(tier("some-sovereign", "norway", 4), Some("sovereign"));
        // A floor can never lower what the residency already implies.
        assert_eq!(tier("azure-openai", "eu", 1), Some("eu_resident"));
        // Out-of-range ordinals express no floor at all.
        assert_eq!(tier("azure-openai", "eu", 99), Some("eu_resident"));
        assert_eq!(tier("azure-openai", "eu", -1), Some("eu_resident"));
        // An undeclared residency asserts nothing by itself.
        assert_eq!(tier("mystery", "", 0), None);
        assert_eq!(tier("mystery", "atlantis", 0), None);
        assert_eq!(
            ServedBy::stamped("mystery", "", 0).map(|served| served.residency),
            Some(String::new())
        );
        // No serving provider, no provenance: a synthetic answer that made
        // no provider call, or an inference-core that stamped nothing.
        assert_eq!(ServedBy::stamped("", "eu", 3), None);
        assert_eq!(ServedBy::stamped("   ", "global", 2), None);
    }

    /// `turn_timing` rides with `usage`: a client that did not ask for usage
    /// never sees it, live or on resume.
    #[test]
    fn turn_timing_is_gated_exactly_like_usage() {
        let timing = crate::turn_timing::TurnTiming::start("req-t");
        let report = timing
            .report(crate::turn_timing::Outcome::Completed)
            .unwrap();
        let event = ChatEvent::TurnTiming { report };
        let usage = ChatEvent::Usage {
            input_tokens: 0,
            output_tokens: 0,
            cost_usd: None,
            cost_basis: None,
            latency_ms: 0,
            confidence: None,
            served_by: None,
        };
        assert_eq!(event.name(), "turn_timing");
        assert_eq!(event.family(), usage.family());
        for features in [
            families(&[]),
            families(&["tools", "steps", "artifacts"]),
            families(&["usage"]),
        ] {
            assert_eq!(
                event.should_emit(&features),
                usage.should_emit(&features),
                "{features:?}"
            );
        }
        let payload = event.payload("ignored");
        assert_eq!(payload["request_id"], "req-t");
        assert_eq!(payload["outcome"], "completed");
        assert!(payload["phases"].is_array());
    }
}

#[cfg(test)]
mod memory_recall_payload_tests {
    use super::ChatEvent;
    use crate::memory_provenance::{MemoryOrigin, MemoryRole, RecalledMemoryView};

    fn view(origin: MemoryOrigin, role: MemoryRole) -> RecalledMemoryView {
        RecalledMemoryView {
            memory_id: "mem-1".to_owned(),
            role,
            origin,
            label: "USER".to_owned(),
            preview: "Prefers metric units".to_owned(),
        }
    }

    fn payload(memories: Vec<RecalledMemoryView>) -> serde_json::Value {
        ChatEvent::MemoryRecall {
            count: memories.len() as u32,
            latency_ms: 12,
            memories,
        }
        .payload("req-1")
    }

    #[test]
    fn the_wire_carries_what_was_recalled_not_only_how_much() {
        let body = payload(vec![view(MemoryOrigin::Stated, MemoryRole::Recall)]);
        assert_eq!(body["count"], 1);
        let memory = &body["memories"][0];
        assert_eq!(memory["memory_id"], "mem-1");
        assert_eq!(memory["label"], "USER");
        assert_eq!(memory["preview"], "Prefers metric units");
        // The id is on the wire so a reader can act on the exact row.
        assert!(memory["memory_id"].is_string());
    }

    /// The honesty rule, asserted at the boundary a client reads: three origin
    /// states, and "unrecorded" must never serialize as "stated".
    #[test]
    fn unrecorded_origin_is_distinguishable_on_the_wire() {
        for (origin, expected) in [
            (MemoryOrigin::Stated, "stated"),
            (MemoryOrigin::Inferred, "inferred"),
            (MemoryOrigin::Unrecorded, "unrecorded"),
        ] {
            let body = payload(vec![view(origin, MemoryRole::Recall)]);
            assert_eq!(body["memories"][0]["origin"], expected);
        }
    }

    #[test]
    fn the_role_split_survives_serialization() {
        let body = payload(vec![
            view(MemoryOrigin::Stated, MemoryRole::Recall),
            view(MemoryOrigin::Stated, MemoryRole::Inject),
        ]);
        assert_eq!(body["memories"][0]["role"], "recall");
        assert_eq!(body["memories"][1]["role"], "inject");
    }

    /// A turn that recalled nothing describable still reports its count, so an
    /// empty list never reads as "memory was not used".
    #[test]
    fn an_empty_description_list_is_still_a_valid_event() {
        let body = payload(Vec::new());
        assert_eq!(body["memories"].as_array().map(Vec::len), Some(0));
        assert!(body["latency_ms"].is_number());
    }
}
